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
        let expanded = self.expand_action(action, actuator)?;
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

        if !matches!(action, Action::SystemdSetProperty { .. }) {
            if let Some(outcome) =
                self.active_non_systemd_outcome(action, actuator, &expanded)?
            {
                self.record_action_outcome(action, &outcome, actuator)?;
                return Ok(outcome);
            }
        }

        let handles = match self.prepare_transactions(actuator, action, &expanded) {
            Ok(handles) => handles,
            Err(error) => {
                let mut outcome = active_action_outcome(action);
                outcome.gates.push(GateEvaluation::denied(
                    GateStage::RecoveryJournal,
                    GateReasonCode::JournalFailed,
                    error.to_string(),
                ));
                for desired in &expanded {
                    outcome.targets.push(TargetOutcome::denied(
                        desired.target_id.clone(),
                        PipelineStage::Journal,
                        error.to_string(),
                    ));
                }
                return Ok(outcome);
            }
        };

        let apply_result = match action {
            Action::SystemdSetProperty {
                unit, properties, ..
            } => self.apply_systemd_action(actuator, action, unit, properties),
            _ => actuator.apply(action),
        };
        let mut outcome = match apply_result {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Err(compensation) = self.compensate_all(actuator, &handles) {
                    return Err(io::Error::other(format!(
                        "apply failed: {error}; S2D compensation failed: {compensation}"
                    )));
                }
                return Err(error);
            }
        };

        if let Err(finalization) =
            self.finalize_transactions(actuator, &expanded, &handles, &mut outcome)
        {
            if let Err(compensation) = self.compensate_all(actuator, &handles) {
                return Err(io::Error::other(format!(
                    "S2D finalization failed: {finalization}; compensation failed: {compensation}"
                )));
            }
            return Err(finalization.into());
        }
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
                match self.transactions
                    .validate_handback(actuator.kernel.as_ref(), &plan.target_id)
                    .map_err(io::Error::from)?
                {
                    HandbackTarget::Present => {
                        actuator.execute_restore(&plan, self.systemd.as_ref())?
                    }
                    HandbackTarget::Removed => relinquished_outcome(
                        &plan.target_id,
                        "target disappeared before restoration; no write attempted",
                    ),
                }
            };
            self.record_restore_outcome(&plan, &outcome, actuator.kernel.as_ref())?;
            outcomes.push(outcome);
        }
        self.persist(actuator.kernel.as_ref())?;
        notify_cycle_complete(&self.transactions, actuator.kernel.as_ref())
            .map_err(io::Error::from)?;
        Ok(outcomes)
    }

    pub(crate) fn restore_all_owned(
        &mut self,
        actuator: &mut Actuator,
    ) -> io::Result<Vec<RestoreOutcome>> {
        self.mark_all_for_restore();
        self.reconcile(actuator)
    }

    pub(crate) fn mark_domain_for_restore(&mut self, domain: &str) {
        for state in self.targets.values_mut() {
            if state.domain == domain {
                state.desired = None;
            }
        }
    }

    pub(crate) fn mark_all_for_restore(&mut self) {
        for state in self.targets.values_mut() {
            state.desired = None;
        }
    }

    pub(crate) fn domain_for_target(&self, target_id: &str) -> Option<&str> {
        self.targets.get(target_id).map(|state| state.domain.as_str())
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

    fn active_non_systemd_outcome(
        &self,
        action: &Action,
        actuator: &mut Actuator,
        targets: &[DesiredTarget],
    ) -> io::Result<Option<ActionOutcome>> {
        if targets.is_empty() {
            return Ok(None);
        }

        if targets.iter().any(|desired| {
            self.targets
                .get(&desired.target_id)
                .is_some_and(|state| state.ownership == OwnershipState::Relinquished)
        }) {
            let mut outcome = active_action_outcome(action);
            for desired in targets {
                let ownership = self
                    .targets
                    .get(&desired.target_id)
                    .map(|state| state.ownership.clone())
                    .unwrap_or(OwnershipState::Unknown);
                outcome.targets.push(TargetOutcome {
                    target_id: desired.target_id.clone(),
                    pipeline_stage: PipelineStage::Readback,
                    support: SupportState::Supported,
                    reason: OutcomeReasonCode::OwnershipRelinquished,
                    write_attempted: false,
                    write_outcome: WriteOutcome::OwnershipRelinquished,
                    readback: ReadbackOutcome::NotPerformed,
                    ownership,
                    pending_restore: RestoreState::NotApplicable,
                    responsible_subsystem: ResponsibleSubsystem::Restoration,
                    detail: Some(
                        "ownership was previously relinquished; desired value was not reasserted"
                            .to_string(),
                    ),
                });
            }
            return Ok(Some(outcome));
        }

        let all_confirmed_owned = targets.iter().all(|desired| {
            self.targets.get(&desired.target_id).is_some_and(|state| {
                state.ownership == OwnershipState::Optid
                    && state.last_confirmed.as_ref() == Some(&desired.desired)
            })
        });
        if !all_confirmed_owned {
            return Ok(None);
        }

        let mut outcome = active_action_outcome(action);
        for desired in targets {
            match self.read_target(actuator, &desired.target) {
                Ok(current) if current == desired.desired => {
                    outcome.targets.push(TargetOutcome {
                        target_id: desired.target_id.clone(),
                        pipeline_stage: PipelineStage::Readback,
                        support: SupportState::Supported,
                        reason: OutcomeReasonCode::RedundantValue,
                        write_attempted: false,
                        write_outcome: WriteOutcome::Redundant,
                        readback: ReadbackOutcome::Confirmed {
                            value: current.public_value(),
                        },
                        ownership: OwnershipState::Optid,
                        pending_restore: RestoreState::Pending,
                        responsible_subsystem: ResponsibleSubsystem::Restoration,
                        detail: Some(
                            "complete desired state remains confirmed; write coalesced".to_string(),
                        ),
                    });
                }
                Ok(current) => {
                    outcome.targets.push(TargetOutcome {
                        target_id: desired.target_id.clone(),
                        pipeline_stage: PipelineStage::Readback,
                        support: SupportState::Supported,
                        reason: OutcomeReasonCode::OwnershipRelinquished,
                        write_attempted: false,
                        write_outcome: WriteOutcome::OwnershipRelinquished,
                        readback: ReadbackOutcome::Mismatch {
                            expected: desired.desired.public_value(),
                            actual: current.public_value(),
                        },
                        ownership: OwnershipState::Relinquished,
                        pending_restore: RestoreState::NotApplicable,
                        responsible_subsystem: ResponsibleSubsystem::Restoration,
                        detail: Some(
                            "external drift detected while target remained desired; write refused"
                                .to_string(),
                        ),
                    });
                }
                Err(error) => {
                    outcome.targets.push(TargetOutcome {
                        target_id: desired.target_id.clone(),
                        pipeline_stage: PipelineStage::Readback,
                        support: SupportState::Supported,
                        reason: OutcomeReasonCode::ReadbackUnavailable,
                        write_attempted: false,
                        write_outcome: WriteOutcome::Skipped,
                        readback: ReadbackOutcome::Unavailable,
                        ownership: OwnershipState::Optid,
                        pending_restore: RestoreState::Pending,
                        responsible_subsystem: ResponsibleSubsystem::KernelIo,
                        detail: Some(format!(
                            "active ownership readback failed: {:?}",
                            error.kind()
                        )),
                    });
                }
            }
        }
        Ok(Some(outcome))
    }

    fn record_action_outcome(
        &mut self,
        action: &Action,
        outcome: &ActionOutcome,
        actuator: &mut Actuator,
    ) -> io::Result<()> {
        let desired_targets = self.expand_action(action, actuator)?;
        let mut relinquished_transactions = BTreeSet::new();
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
            if target_outcome.ownership == OwnershipState::Relinquished {
                relinquished_transactions.insert(desired.target_id.clone());
            }
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
        for target_id in relinquished_transactions {
            self.transactions
                .finish_handback(actuator.kernel.as_ref(), &target_id, true)
                .map_err(io::Error::from)?;
        }
        self.persist(actuator.kernel.as_ref())
    }

    fn record_restore_outcome(
        &mut self,
        plan: &RestorePlan,
        outcome: &RestoreOutcome,
        io: &dyn KernelIo,
    ) -> io::Result<()> {
        let mut finish_transaction = None;
        {
            let Some(state) = self.targets.get_mut(&plan.target_id) else {
                return Ok(());
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
                    finish_transaction = Some(false);
                }
                WriteOutcome::OwnershipRelinquished => {
                    state.ownership = OwnershipState::Relinquished;
                    state.ownership_reason = outcome.detail.clone();
                    state.restore_pending = false;
                    finish_transaction = Some(true);
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
        if let Some(relinquished) = finish_transaction {
            self.transactions
                .finish_handback(io, &plan.target_id, relinquished)
                .map_err(io::Error::from)?;
        }
        Ok(())
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

fn active_action_outcome(action: &Action) -> ActionOutcome {
    let mut outcome = ActionOutcome::new(action);
    outcome.gates.push(GateEvaluation::allowed(
        GateStage::DomainMode,
        GateReasonCode::DomainActuate,
    ));
    outcome.gates.push(GateEvaluation::allowed(
        GateStage::ApplyArmed,
        GateReasonCode::ApplyArmed,
    ));
    outcome
}
