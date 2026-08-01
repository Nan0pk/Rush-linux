impl Reconciler {
    fn expand_action(
        &self,
        action: &Action,
        actuator: &mut Actuator,
    ) -> io::Result<Vec<DesiredTarget>> {
        let domain = action
            .domain()
            .map(|domain| domain.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let legacy = action.journal_key();
        let targets = match action {
            Action::CpuEpp { value, .. } => discover_cpu_epp_paths_with(actuator.kernel.as_ref())
                .into_iter()
                .map(|path| DesiredTarget {
                    target_id: action.stable_expanded_target_id(&path),
                    domain: domain.clone(),
                    target: TargetKind::KernelValue { path },
                    desired: StoredValue::Scalar {
                        value: value.clone(),
                    },
                    legacy_journal_key: None,
                })
                .collect(),
            Action::PlatformProfile { value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue {
                    path: PathBuf::from("/sys/firmware/acpi/platform_profile"),
                },
                desired: StoredValue::Scalar {
                    value: value.clone(),
                },
                legacy_journal_key: None,
            }],
            Action::SystemdSetProperty {
                unit, properties, ..
            } => properties
                .iter()
                .filter_map(|assignment| assignment.split_once('='))
                .map(|(property, value)| DesiredTarget {
                    target_id: format!(
                        "{}:property:{}",
                        action.stable_target_id(),
                        sanitize_identity(property)
                    ),
                    domain: domain.clone(),
                    target: TargetKind::SystemdProperty {
                        unit: unit.clone(),
                        property: property.to_string(),
                    },
                    desired: StoredValue::Systemd {
                        explicit: true,
                        value: value.to_string(),
                    },
                    legacy_journal_key: None,
                })
                .collect(),
            Action::VmSysctl { path, value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue { path: path.clone() },
                desired: StoredValue::Scalar {
                    value: value.clone(),
                },
                legacy_journal_key: legacy,
            }],
            Action::CpuDmaLatency { value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::PmqosCpu,
                desired: StoredValue::Scalar {
                    value: value
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unconstrained".to_string()),
                },
                legacy_journal_key: legacy,
            }],
            Action::DeviceResumeLatency { path, value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::PmqosDevice { path: path.clone() },
                desired: StoredValue::Scalar {
                    value: value
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "0".to_string()),
                },
                legacy_journal_key: legacy,
            }],
            Action::RuntimePm {
                device_dir,
                autosuspend_delay_ms,
                ..
            } => {
                let delay_path = device_dir.join("power/autosuspend_delay_ms");
                let has_delay = actuator.kernel.exists(&delay_path);
                vec![DesiredTarget {
                    target_id: action.stable_target_id(),
                    domain,
                    target: TargetKind::RuntimePm {
                        control_path: device_dir.join("power/control"),
                        delay_path: has_delay.then_some(delay_path),
                    },
                    desired: StoredValue::RuntimePm {
                        control: "auto".to_string(),
                        delay: has_delay.then(|| autosuspend_delay_ms.to_string()),
                    },
                    legacy_journal_key: legacy,
                }]
            }
            Action::PcieAspm {
                device_dir, enable, ..
            } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue {
                    path: device_dir.join("link/l1_aspm"),
                },
                desired: StoredValue::Scalar {
                    value: if *enable { "1" } else { "0" }.to_string(),
                },
                legacy_journal_key: legacy,
            }],
            Action::SataAlpm {
                host_dir, policy, ..
            } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue {
                    path: host_dir.join("link_power_management_policy"),
                },
                desired: StoredValue::Scalar {
                    value: policy.clone(),
                },
                legacy_journal_key: legacy,
            }],
            Action::Backlight {
                device_dir,
                target_pct,
                ..
            } => {
                let max_path = device_dir.join("max_brightness");
                let max = actuator
                    .kernel
                    .read_to_string(&max_path)?
                    .trim()
                    .parse::<u64>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let value = display::compute_target_brightness(max, *target_pct).to_string();
                vec![DesiredTarget {
                    target_id: action.stable_target_id(),
                    domain,
                    target: TargetKind::KernelValue {
                        path: device_dir.join("brightness"),
                    },
                    desired: StoredValue::Scalar { value },
                    legacy_journal_key: legacy,
                }]
            }
        };
        Ok(targets)
    }

    fn apply_systemd_action(
        &mut self,
        _actuator: &mut Actuator,
        action: &Action,
        unit: &str,
        properties: &[String],
    ) -> io::Result<ActionOutcome> {
        let mut outcome = ActionOutcome::new(action);
        outcome.gates.push(GateEvaluation::allowed(
            GateStage::DomainMode,
            GateReasonCode::DomainActuate,
        ));
        outcome.gates.push(GateEvaluation::allowed(
            GateStage::ApplyArmed,
            GateReasonCode::ApplyArmed,
        ));
        outcome.gates.push(GateEvaluation::not_applicable(
            GateStage::Contract,
            GateReasonCode::ContractNotApplicable,
        ));
        outcome.gates.push(GateEvaluation::not_applicable(
            GateStage::CapabilityValidation,
            GateReasonCode::CapabilityAllowed,
        ));
        for assignment in properties {
            let Some((property, value)) = assignment.split_once('=') else {
                let error = io::Error::new(io::ErrorKind::InvalidInput, "invalid systemd property");
                outcome.targets.push(TargetOutcome::failed(
                    format!("{}:property:invalid", action.stable_target_id()),
                    &error,
                ));
                continue;
            };
            let target_id = format!(
                "{}:property:{}",
                action.stable_target_id(),
                sanitize_identity(property)
            );
            let desired = StoredValue::Systemd {
                explicit: true,
                value: value.to_string(),
            };
            let (ownership, last_confirmed, has_baseline) = self
                .targets
                .get(&target_id)
                .map(|state| {
                    (
                        state.ownership.clone(),
                        state.last_confirmed.clone(),
                        state.baseline.is_some(),
                    )
                })
                .unwrap_or((OwnershipState::Unknown, None, false));
            if !has_baseline {
                outcome.targets.push(systemd_failed_target(
                    target_id,
                    false,
                    PipelineStage::Journal,
                    &io::Error::new(
                        io::ErrorKind::InvalidData,
                        "systemd baseline capture failed",
                    ),
                ));
                continue;
            }
            if ownership == OwnershipState::Relinquished {
                outcome.targets.push(systemd_relinquished_target(
                    target_id,
                    ReadbackOutcome::NotPerformed,
                    "ownership was previously relinquished; desired value was not reasserted",
                ));
                continue;
            }
            let current = match self.systemd.read_property(unit, property) {
                Ok(current) => current,
                Err(error) => {
                    outcome.targets.push(systemd_failed_target(
                        target_id,
                        false,
                        PipelineStage::Readback,
                        &error,
                    ));
                    continue;
                }
            };
            let current_value = StoredValue::Systemd {
                explicit: current.explicit,
                value: current.value.clone(),
            };
            if ownership == OwnershipState::Optid
                && last_confirmed.as_ref() == Some(&desired)
                && current_value != desired
            {
                outcome.targets.push(systemd_relinquished_target(
                    target_id,
                    ReadbackOutcome::Mismatch {
                        expected: desired.public_value(),
                        actual: current_value.public_value(),
                    },
                    "external drift detected while property remained desired; write refused",
                ));
                continue;
            }
            if current.explicit && current.value == value {
                outcome.targets.push(TargetOutcome {
                    target_id: target_id.clone(),
                    pipeline_stage: PipelineStage::Write,
                    support: SupportState::Supported,
                    reason: OutcomeReasonCode::RedundantValue,
                    write_attempted: false,
                    write_outcome: WriteOutcome::Redundant,
                    readback: ReadbackOutcome::Confirmed {
                        value: current.value,
                    },
                    ownership,
                    pending_restore: RestoreState::Pending,
                    responsible_subsystem: ResponsibleSubsystem::Systemd,
                    detail: None,
                });
                continue;
            }
            match self.systemd.set_property(unit, property, Some(value)) {
                Ok(()) => {
                    let readback = match self.systemd.read_property(unit, property) {
                        Ok(readback) => readback,
                        Err(error) => {
                            outcome.targets.push(systemd_failed_target(
                                target_id,
                                true,
                                PipelineStage::Readback,
                                &error,
                            ));
                            continue;
                        }
                    };
                    let confirmed = readback.explicit && readback.value == value;
                    outcome.targets.push(TargetOutcome {
                        target_id,
                        pipeline_stage: PipelineStage::Readback,
                        support: SupportState::Supported,
                        reason: if confirmed {
                            OutcomeReasonCode::ReadbackConfirmed
                        } else {
                            OutcomeReasonCode::ReadbackMismatch
                        },
                        write_attempted: true,
                        write_outcome: WriteOutcome::Applied,
                        readback: if confirmed {
                            ReadbackOutcome::Confirmed {
                                value: readback.value,
                            }
                        } else {
                            ReadbackOutcome::Mismatch {
                                expected: value.to_string(),
                                actual: readback.value,
                            }
                        },
                        ownership: if confirmed {
                            OwnershipState::Optid
                        } else {
                            OwnershipState::Drifted
                        },
                        pending_restore: RestoreState::Pending,
                        responsible_subsystem: ResponsibleSubsystem::Systemd,
                        detail: None,
                    });
                }
                Err(error) => outcome.targets.push(systemd_failed_target(
                    target_id,
                    true,
                    PipelineStage::Write,
                    &error,
                )),
            }
        }
        Ok(outcome)
    }

    fn read_target(&self, actuator: &mut Actuator, target: &TargetKind) -> io::Result<StoredValue> {
        match target {
            TargetKind::KernelValue { path } => Ok(StoredValue::Scalar {
                value: actuator.kernel.read_to_string(path)?.trim().to_string(),
            }),
            TargetKind::PmqosCpu => Ok(StoredValue::Scalar {
                value: actuator.pmqos_sink.read_cpu_latency()?.trim().to_string(),
            }),
            TargetKind::PmqosDevice { path } => Ok(StoredValue::Scalar {
                value: actuator
                    .pmqos_sink
                    .read_device_latency(path)?
                    .trim()
                    .to_string(),
            }),
            TargetKind::RuntimePm {
                control_path,
                delay_path,
            } => Ok(StoredValue::RuntimePm {
                control: actuator
                    .kernel
                    .read_to_string(control_path)?
                    .trim()
                    .to_string(),
                delay: delay_path
                    .as_ref()
                    .map(|path| {
                        actuator
                            .kernel
                            .read_to_string(path)
                            .map(|value| value.trim().to_string())
                    })
                    .transpose()?,
            }),
            TargetKind::SystemdProperty { unit, property } => {
                let state = self.systemd.read_property(unit, property)?;
                Ok(StoredValue::Systemd {
                    explicit: state.explicit,
                    value: state.value,
                })
            }
        }
    }

    fn persist(&self, io: &dyn KernelIo) -> io::Result<()> {
        let content = serde_json::to_string_pretty(&PersistedState {
            schema_version: 1,
            targets: self.targets.clone(),
        })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        atomic_write_state_file_with(io, &self.state_dir.join(STATE_FILE), &content)
    }

    fn hydrate_legacy(&mut self, actuator: &mut Actuator) -> io::Result<()> {
        let entries = match actuator.kernel.read_dir(&self.state_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for original_path in entries {
            let Some(name) = original_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(key) = name.strip_prefix("original_") else {
                continue;
            };
            if self
                .targets
                .values()
                .any(|state| state.legacy_journal_key.as_deref() == Some(key))
            {
                continue;
            }
            let Some((target_id, domain, target, baseline)) =
                parse_legacy_original(actuator.kernel.as_ref(), key, &original_path)?
            else {
                continue;
            };
            let applied_path = self.state_dir.join(format!("applied_{key}"));
            let last_confirmed = actuator
                .kernel
                .read_to_string(&applied_path)
                .ok()
                .and_then(|content| parse_legacy_applied(key, &content));
            let current = self.read_target(actuator, &target).ok();
            let ownership = if last_confirmed.is_some() && current == last_confirmed {
                OwnershipState::Optid
            } else {
                OwnershipState::Unowned
            };
            self.targets.insert(
                target_id.clone(),
                TargetState {
                    target_id,
                    domain,
                    target,
                    legacy_journal_key: Some(key.to_string()),
                    baseline: Some(baseline),
                    desired: None,
                    last_attempted: last_confirmed.clone(),
                    last_confirmed,
                    ownership: ownership.clone(),
                    ownership_reason: None,
                    retries: 0,
                    restore_pending: ownership == OwnershipState::Optid,
                },
            );
        }
        Ok(())
    }
}

fn systemd_relinquished_target(
    target_id: String,
    readback: ReadbackOutcome,
    detail: &str,
) -> TargetOutcome {
    TargetOutcome {
        target_id,
        pipeline_stage: PipelineStage::Readback,
        support: SupportState::Supported,
        reason: OutcomeReasonCode::OwnershipRelinquished,
        write_attempted: false,
        write_outcome: WriteOutcome::OwnershipRelinquished,
        readback,
        ownership: OwnershipState::Relinquished,
        pending_restore: RestoreState::NotApplicable,
        responsible_subsystem: ResponsibleSubsystem::Systemd,
        detail: Some(detail.to_string()),
    }
}
