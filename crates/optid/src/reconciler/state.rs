impl Reconciler {
    pub(crate) fn load(state_dir: PathBuf, actuator: &mut Actuator) -> io::Result<Self> {
        let recovery_dir = default_recovery_dir(&state_dir);
        Self::load_with_systemd_and_recovery(
            state_dir,
            recovery_dir,
            actuator,
            Box::<RealSystemd>::default(),
        )
    }

    #[cfg(test)]
    fn load_with_systemd(
        state_dir: PathBuf,
        actuator: &mut Actuator,
        systemd: Box<dyn SystemdIo>,
    ) -> io::Result<Self> {
        let recovery_dir = default_recovery_dir(&state_dir);
        Self::load_with_systemd_and_recovery(
            state_dir,
            recovery_dir,
            actuator,
            systemd,
        )
    }

    fn load_with_systemd_and_recovery(
        state_dir: PathBuf,
        recovery_dir: PathBuf,
        actuator: &mut Actuator,
        systemd: Box<dyn SystemdIo>,
    ) -> io::Result<Self> {
        let mode = ReconcileMode::load(actuator.kernel.as_ref(), &state_dir);
        let targets = match actuator.kernel.read_to_string(&state_dir.join(STATE_FILE)) {
            Ok(content) => serde_json::from_str::<PersistedState>(&content)
                .map(|state| state.targets)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error),
        };
        let mut reconciler = Self {
            state_dir,
            mode,
            targets,
            previous_desired: BTreeSet::new(),
            last_ac: None,
            last_workload: WorkloadClass::Idle,
            last_mode: Mode::Auto,
            last_domain_modes: HashMap::new(),
            transactions: TransactionEngine::for_process(recovery_dir),
            systemd,
        };
        reconciler.hydrate_legacy(actuator)?;
        reconciler.persist(actuator.kernel.as_ref())?;
        Ok(reconciler)
    }

    pub(crate) fn mode(&self) -> ReconcileMode {
        self.mode
    }

    pub(crate) fn detect_transitions(
        &mut self,
        on_ac: Option<bool>,
        workload: WorkloadClass,
        mode: Mode,
        domain_modes: &HashMap<Domain, DomainMode>,
    ) -> Vec<Transition> {
        let mut transitions = Vec::new();
        if self.last_ac.is_some() && self.last_ac != on_ac {
            transitions.push(Transition::AcChanged {
                from: self.last_ac,
                to: on_ac,
            });
        }
        self.last_ac = on_ac;
        if self.last_workload != workload {
            transitions.push(Transition::WorkloadChanged {
                from: self.last_workload,
                to: workload,
            });
        }
        self.last_workload = workload;
        if self.last_mode != mode {
            transitions.push(Transition::ModeChanged {
                from: self.last_mode,
                to: mode,
            });
        }
        self.last_mode = mode;
        for (domain, current) in domain_modes {
            if self
                .last_domain_modes
                .get(domain)
                .is_some_and(|previous| *previous != DomainMode::Off && *current == DomainMode::Off)
            {
                transitions.push(Transition::DomainDisabled { domain: *domain });
            }
            self.last_domain_modes.insert(*domain, *current);
        }
        transitions
    }

    pub(crate) fn prepare_cycle(
        &mut self,
        actions: &[Action],
        actuator: &mut Actuator,
    ) -> io::Result<Vec<String>> {
        self.previous_desired = self
            .targets
            .iter()
            .filter(|(_, state)| state.desired.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for state in self.targets.values_mut() {
            state.desired = None;
        }

        let mut desired_ids = BTreeSet::new();
        for action in actions {
            for desired in self.expand_action(action, actuator)? {
                desired_ids.insert(desired.target_id.clone());
                let may_capture_baseline = self
                    .targets
                    .get(&desired.target_id)
                    .map(|state| {
                        state.baseline.is_none()
                            && state.ownership == OwnershipState::Unowned
                            && state.last_confirmed.is_none()
                    })
                    .unwrap_or(true);
                let baseline = if may_capture_baseline {
                    self.read_target(actuator, &desired.target).ok()
                } else {
                    None
                };
                let state = self
                    .targets
                    .entry(desired.target_id.clone())
                    .or_insert_with(|| TargetState {
                        target_id: desired.target_id.clone(),
                        domain: desired.domain.clone(),
                        target: desired.target.clone(),
                        legacy_journal_key: desired.legacy_journal_key.clone(),
                        baseline: baseline.clone(),
                        desired: None,
                        last_attempted: None,
                        last_confirmed: None,
                        ownership: OwnershipState::Unowned,
                        ownership_reason: None,
                        retries: 0,
                        restore_pending: false,
                    });
                state.domain = desired.domain;
                state.target = desired.target;
                state.legacy_journal_key = desired.legacy_journal_key;
                if may_capture_baseline && state.baseline.is_none() {
                    state.baseline = baseline;
                }
                if state.ownership == OwnershipState::Optid
                    && state.restore_pending
                    && state.retries > 0
                {
                    // Renewed demand must not cancel a failed handback or
                    // reset its bounded retry count. Restore before reapply.
                    continue;
                }
                if state.desired.as_ref() != Some(&desired.desired) {
                    state.retries = 0;
                }
                state.desired = Some(desired.desired);
            }
        }
        self.persist(actuator.kernel.as_ref())?;

        Ok(self
            .previous_desired
            .difference(&desired_ids)
            .cloned()
            .collect())
    }
}
