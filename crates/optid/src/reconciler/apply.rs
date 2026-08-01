impl Reconciler {
    pub(crate) fn apply_action(
        &mut self,
        actuator: &mut Actuator,
        action: &Action,
    ) -> io::Result<ActionOutcome> {
        if actuator
            .boot_state
            .as_ref()
            .is_some_and(|boot| !boot.apply_armed)
        {
            let mut outcome = ActionOutcome::new(action);
            outcome.gates.push(GateEvaluation::allowed(
                GateStage::DomainMode,
                GateReasonCode::DomainActuate,
            ));
            outcome.gates.push(GateEvaluation::denied(
                GateStage::ApplyArmed,
                GateReasonCode::ApplyDisarmedByBootState,
                "dynamic writes are disarmed",
            ));
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::ApplyGate,
                "dynamic writes are disarmed".to_string(),
            ));
            return Ok(outcome);
        }
        let expanded = self.expand_action(action, actuator).unwrap_or_default();
        if !matches!(action, Action::SystemdSetProperty { .. })
            && expanded.iter().any(|desired| {
                self.targets
                    .get(&desired.target_id)
                    .map_or(true, |state| state.baseline.is_none())
            })
        {
            let mut outcome = ActionOutcome::new(action);
            outcome.gates.push(GateEvaluation::allowed(
                GateStage::DomainMode,
                GateReasonCode::DomainActuate,
            ));
            outcome.gates.push(GateEvaluation::denied(
                GateStage::RecoveryJournal,
                GateReasonCode::JournalFailed,
                "baseline capture failed; write refused",
            ));
            for desired in expanded {
                outcome.targets.push(TargetOutcome::denied(
                    desired.target_id,
                    PipelineStage::Journal,
                    "baseline capture failed; write refused".to_string(),
                ));
            }
            return Ok(outcome);
        }

        let outcome = match action {
            Action::SystemdSetProperty {
                unit, properties, ..
            } => self.apply_systemd_action(actuator, action, unit, properties)?,
            _ if self.action_is_coalesced(action, actuator) => {
                let mut outcome = ActionOutcome::new(action);
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::DomainMode,
                    GateReasonCode::DomainActuate,
                ));
                outcome.targets.push(TargetOutcome {
                    target_id: action.stable_target_id(),
                    pipeline_stage: PipelineStage::Write,
                    support: SupportState::Supported,
                    reason: OutcomeReasonCode::RedundantValue,
                    write_attempted: false,
                    write_outcome: WriteOutcome::Redundant,
                    readback: ReadbackOutcome::NotPerformed,
                    ownership: OwnershipState::Optid,
                    pending_restore: RestoreState::Pending,
                    responsible_subsystem: ResponsibleSubsystem::Restoration,
                    detail: Some("complete desired state already confirmed".to_string()),
                });
                outcome
            }
            _ => actuator.apply(action)?,
        };
        self.record_action_outcome(action, &outcome, actuator)?;
        Ok(outcome)
    }

    pub(crate) fn reconcile(&mut self, actuator: &mut Actuator) -> io::Result<Vec<RestoreOutcome>> {
        let plans = self.plan_restores();
        let mut outcomes = Vec::with_capacity(plans.len());
        for plan in plans {
            let outcome = if self.mode == ReconcileMode::Shadow {
                RestoreOutcome {
                    target_id: plan.target_id.clone(),
                    pipeline_stage: PipelineStage::Restore,
                    reason: OutcomeReasonCode::NotEvaluated,
                    write_attempted: false,
                    write_outcome: WriteOutcome::NotEvaluated,
                    readback: ReadbackOutcome::NotPerformed,
                    ownership: OwnershipState::Optid,
                    pending_restore: RestoreState::Pending,
                    responsible_subsystem: ResponsibleSubsystem::Restoration,
                    detail: Some("shadow restore plan; no write executed".to_string()),
                }
            } else {
                actuator.execute_restore(&plan, self.systemd.as_ref())?
            };
            self.record_restore_outcome(&plan, &outcome, actuator.kernel.as_ref());
            outcomes.push(outcome);
        }
        self.persist(actuator.kernel.as_ref())?;
        Ok(outcomes)
    }

    pub(crate) fn restore_all_owned(
        &mut self,
        actuator: &mut Actuator,
    ) -> io::Result<Vec<RestoreOutcome>> {
        for state in self.targets.values_mut() {
            state.desired = None;
        }
        self.reconcile(actuator)
    }

    pub(crate) fn parity_report(&self, legacy_stale_keys: &BTreeSet<String>) -> ParityReport {
        let plans = self.plan_restores();
        let planned: BTreeSet<String> = plans
            .iter()
            .filter_map(|plan| plan.legacy_journal_key.clone())
            .collect();
        let comparable_planned: BTreeSet<String> = planned
            .iter()
            .filter(|key| legacy_restore_supported(key))
            .cloned()
            .collect();
        let comparable_legacy: BTreeSet<String> = legacy_stale_keys
            .iter()
            .filter(|key| legacy_restore_supported(key))
            .cloned()
            .collect();
        ParityReport {
            legacy: comparable_legacy.clone(),
            v1: comparable_planned.clone(),
            parity: comparable_legacy == comparable_planned,
            intentional_v1_only: plans
                .iter()
                .filter(|plan| {
                    plan.legacy_journal_key
                        .as_deref()
                        .map_or(true, |key| !legacy_restore_supported(key))
                })
                .map(|plan| plan.target_id.clone())
                .collect(),
        }
    }

    fn action_is_coalesced(&self, action: &Action, actuator: &mut Actuator) -> bool {
        let Ok(targets) = self.expand_action(action, actuator) else {
            return false;
        };
        !targets.is_empty()
            && targets.iter().all(|target| {
                self.targets.get(&target.target_id).is_some_and(|state| {
                    state.ownership == OwnershipState::Optid
                        && state.last_confirmed.as_ref() == Some(&target.desired)
                })
            })
    }

    fn record_action_outcome(
        &mut self,
        action: &Action,
        outcome: &ActionOutcome,
        actuator: &mut Actuator,
    ) -> io::Result<()> {
        let desired_targets = self.expand_action(action, actuator)?;
        let by_id: HashMap<&str, &TargetOutcome> = outcome
            .targets
            .iter()
            .map(|target| (target.target_id.as_str(), target))
            .collect();
        for desired in desired_targets {
            let Some(state) = self.targets.get_mut(&desired.target_id) else {
                continue;
            };
            let target_outcome = by_id
                .get(desired.target_id.as_str())
                .copied()
                .or_else(|| outcome.targets.first());
            let Some(target_outcome) = target_outcome else {
                continue;
            };
            if target_outcome.write_attempted {
                state.last_attempted = Some(desired.desired.clone());
            }
            match (&target_outcome.readback, target_outcome.write_attempted) {
                (ReadbackOutcome::Confirmed { .. }, true)
                    if target_outcome.ownership == OwnershipState::Optid =>
                {
                    if state.baseline.is_none() {
                        state.ownership = OwnershipState::Unowned;
                        state.ownership_reason = Some(
                            "write confirmed but baseline is missing; ownership not claimed"
                                .to_string(),
                        );
                        state.restore_pending = false;
                        continue;
                    }
                    state.last_confirmed = Some(desired.desired);
                    state.ownership = OwnershipState::Optid;
                    state.ownership_reason = None;
                    state.restore_pending = true;
                    state.retries = 0;
                }
                (ReadbackOutcome::Mismatch { expected, actual }, _) => {
                    state.ownership = OwnershipState::Relinquished;
                    state.ownership_reason = Some(format!(
                        "apply readback drift: expected {expected}, observed {actual}"
                    ));
                    state.restore_pending = false;
                }
                _ => {}
            }
        }
        self.persist(actuator.kernel.as_ref())
    }

    fn record_restore_outcome(
        &mut self,
        plan: &RestorePlan,
        outcome: &RestoreOutcome,
        io: &dyn KernelIo,
    ) {
        let Some(state) = self.targets.get_mut(&plan.target_id) else {
            return;
        };
        match &outcome.write_outcome {
            WriteOutcome::Restored => {
                state.ownership = OwnershipState::Unowned;
                state.ownership_reason = None;
                state.last_attempted = Some(plan.baseline.clone());
                state.last_confirmed = None;
                state.restore_pending = false;
                state.retries = 0;
                if let Some(key) = &plan.legacy_journal_key {
                    clear_journal_with(io, &self.state_dir, key);
                }
            }
            WriteOutcome::OwnershipRelinquished => {
                state.ownership = OwnershipState::Relinquished;
                state.ownership_reason = outcome.detail.clone();
                state.restore_pending = false;
            }
            WriteOutcome::RestorationFailed { .. } => {
                state.retries = state.retries.saturating_add(1);
                if state.retries >= MAX_RESTORE_RETRIES {
                    state.ownership = OwnershipState::Relinquished;
                    state.ownership_reason = Some(format!(
                        "restore retry limit reached ({MAX_RESTORE_RETRIES})"
                    ));
                    state.restore_pending = false;
                } else {
                    state.restore_pending = true;
                }
            }
            _ => {}
        }
    }

    fn plan_restores(&self) -> Vec<RestorePlan> {
        self.targets
            .values()
            .filter(|state| {
                state.ownership == OwnershipState::Optid
                    && state.desired.is_none()
                    && state.restore_pending
                    && state.retries < MAX_RESTORE_RETRIES
            })
            .filter_map(|state| {
                Some(RestorePlan {
                    target_id: state.target_id.clone(),
                    target: state.target.clone(),
                    baseline: state.baseline.clone()?,
                    last_confirmed: state.last_confirmed.clone()?,
                    legacy_journal_key: state.legacy_journal_key.clone(),
                })
            })
            .collect()
    }
}
