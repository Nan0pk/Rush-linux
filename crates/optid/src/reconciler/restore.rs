impl Actuator {
    pub(crate) fn execute_restore(
        &mut self,
        plan: &RestorePlan,
        systemd: &dyn SystemdIo,
    ) -> io::Result<RestoreOutcome> {
        let current = read_target_for_restore(self, systemd, &plan.target);
        let current = match current {
            Ok(current) => current,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(relinquished_outcome(
                    &plan.target_id,
                    "target disappeared before restoration",
                ));
            }
            Err(error) => return Ok(failed_restore(&plan.target_id, false, &error)),
        };
        if current != plan.last_confirmed {
            return Ok(RestoreOutcome {
                target_id: plan.target_id.clone(),
                pipeline_stage: PipelineStage::Restore,
                reason: OutcomeReasonCode::OwnershipRelinquished,
                write_attempted: false,
                write_outcome: WriteOutcome::OwnershipRelinquished,
                readback: ReadbackOutcome::Mismatch {
                    expected: plan.last_confirmed.public_value(),
                    actual: current.public_value(),
                },
                ownership: OwnershipState::Relinquished,
                pending_restore: RestoreState::NotApplicable,
                responsible_subsystem: ResponsibleSubsystem::Restoration,
                detail: Some("external drift detected; restore refused".to_string()),
            });
        }

        if let Err(error) = write_target_for_restore(self, systemd, &plan.target, &plan.baseline) {
            return Ok(failed_restore(&plan.target_id, true, &error));
        }
        let readback = match read_target_for_restore(self, systemd, &plan.target) {
            Ok(readback) => readback,
            Err(error) => return Ok(failed_restore(&plan.target_id, true, &error)),
        };
        if readback != plan.baseline {
            return Ok(RestoreOutcome {
                target_id: plan.target_id.clone(),
                pipeline_stage: PipelineStage::Readback,
                reason: OutcomeReasonCode::RestoreFailed,
                write_attempted: true,
                write_outcome: WriteOutcome::RestorationFailed {
                    error_kind: ErrorKindCode::Other,
                },
                readback: ReadbackOutcome::Mismatch {
                    expected: plan.baseline.public_value(),
                    actual: readback.public_value(),
                },
                ownership: OwnershipState::Optid,
                pending_restore: RestoreState::Pending,
                responsible_subsystem: ResponsibleSubsystem::Restoration,
                detail: Some("restore readback mismatch".to_string()),
            });
        }
        Ok(RestoreOutcome {
            target_id: plan.target_id.clone(),
            pipeline_stage: PipelineStage::Restore,
            reason: OutcomeReasonCode::RestoreApplied,
            write_attempted: true,
            write_outcome: WriteOutcome::Restored,
            readback: ReadbackOutcome::Confirmed {
                value: readback.public_value(),
            },
            ownership: OwnershipState::Unowned,
            pending_restore: RestoreState::Restored,
            responsible_subsystem: ResponsibleSubsystem::Restoration,
            detail: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParityReport {
    pub(crate) legacy: BTreeSet<String>,
    pub(crate) v1: BTreeSet<String>,
    pub(crate) parity: bool,
    pub(crate) intentional_v1_only: BTreeSet<String>,
}

fn read_target_for_restore(
    actuator: &mut Actuator,
    systemd: &dyn SystemdIo,
    target: &TargetKind,
) -> io::Result<StoredValue> {
    match target {
        TargetKind::KernelValue { path } => Ok(StoredValue::Scalar {
            value: actuator.kernel.read_to_string(path)?.trim().to_string(),
        }),
        TargetKind::PmqosCpu => Ok(StoredValue::Scalar {
            value: actuator.pmqos_sink.read_cpu_latency()?.trim().to_string(),
        }),
        TargetKind::PmqosDevice { path } => Ok(StoredValue::Scalar {
            value: actuator.read_device_latency(path)?
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
            let state = systemd.read_property(unit, property)?;
            Ok(StoredValue::Systemd {
                explicit: state.explicit,
                value: state.value,
            })
        }
    }
}

fn write_target_for_restore(
    actuator: &mut Actuator,
    systemd: &dyn SystemdIo,
    target: &TargetKind,
    value: &StoredValue,
) -> io::Result<()> {
    match (target, value) {
        (TargetKind::KernelValue { path }, StoredValue::Scalar { value }) => {
            actuator.kernel.write(path, value)
        }
        (TargetKind::PmqosCpu, StoredValue::Scalar { value }) => {
            let parsed = if value == "unconstrained" {
                None
            } else {
                Some(
                    value
                        .parse::<i32>()
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
                )
            };
            actuator.pmqos_sink.write_cpu_latency(parsed)
        }
        (TargetKind::PmqosDevice { path }, StoredValue::Scalar { value }) => {
            actuator.write_device_latency(path, value)
        }
        (
            TargetKind::RuntimePm {
                control_path,
                delay_path,
            },
            StoredValue::RuntimePm { control, delay },
        ) => {
            actuator.kernel.write(control_path, control)?;
            if let (Some(path), Some(delay)) = (delay_path, delay) {
                actuator.kernel.write(path, delay)?;
            }
            Ok(())
        }
        (
            TargetKind::SystemdProperty { unit, property },
            StoredValue::Systemd { explicit, value },
        ) => systemd.set_property(unit, property, explicit.then_some(value.as_str())),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "target/value kind mismatch",
        )),
    }
}

fn parse_legacy_original(
    io: &dyn KernelIo,
    key: &str,
    path: &Path,
) -> io::Result<Option<(String, String, TargetKind, StoredValue)>> {
    let content = io.read_to_string(path)?;
    let mut lines = content.lines();
    let parsed = if let Some(hash) = key.strip_prefix("rpm_") {
        let Some(device_dir) = lines.next() else {
            return Ok(None);
        };
        let Some(control) = lines.next() else {
            return Ok(None);
        };
        let delay = lines
            .next()
            .filter(|value| *value != "n/a")
            .map(str::to_string);
        let device_dir = PathBuf::from(device_dir);
        (
            format!("runtime-pm:{hash}"),
            "runtime_pm".to_string(),
            TargetKind::RuntimePm {
                control_path: device_dir.join("power/control"),
                delay_path: delay
                    .as_ref()
                    .map(|_| device_dir.join("power/autosuspend_delay_ms")),
            },
            StoredValue::RuntimePm {
                control: control.trim().to_string(),
                delay,
            },
        )
    } else if let Some(hash) = key.strip_prefix("dev_") {
        let Some(attr) = lines.next() else {
            return Ok(None);
        };
        let Some(value) = lines.next() else {
            return Ok(None);
        };
        (
            format!("device-resume:{hash}"),
            "device_resume_latency".to_string(),
            TargetKind::PmqosDevice {
                path: PathBuf::from(attr),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(hash) = key.strip_prefix("aspm_") {
        let Some(base) = lines.next() else {
            return Ok(None);
        };
        let Some(value) = lines.next() else {
            return Ok(None);
        };
        (
            format!("pcie-aspm:{hash}"),
            "pci_aspm".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(base).join("link/l1_aspm"),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(hash) = key.strip_prefix("alpm_") {
        let Some(base) = lines.next() else {
            return Ok(None);
        };
        let Some(value) = lines.next() else {
            return Ok(None);
        };
        (
            format!("sata-alpm:{hash}"),
            "sata_alpm".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(base).join("link_power_management_policy"),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(hash) = key.strip_prefix("bl_") {
        let Some(base) = lines.next() else {
            return Ok(None);
        };
        let Some(value) = lines.next() else {
            return Ok(None);
        };
        (
            format!("backlight:{hash}"),
            "backlight".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(base).join("brightness"),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(name) = key.strip_prefix("vm_") {
        (
            format!("vm-sysctl:{}", sanitize_identity(name)),
            "vm_sysctl".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(format!("/proc/sys/vm/{name}")),
            },
            StoredValue::Scalar {
                value: content.trim().to_string(),
            },
        )
    } else {
        return Ok(None);
    };
    Ok(Some(parsed))
}

fn parse_legacy_applied(key: &str, content: &str) -> Option<StoredValue> {
    let value = content.split_once('\n')?.1;
    if key.starts_with("rpm_") {
        let mut lines = value.lines();
        Some(StoredValue::RuntimePm {
            control: lines.next()?.trim().to_string(),
            delay: lines.next().map(|value| value.trim().to_string()),
        })
    } else {
        Some(StoredValue::Scalar {
            value: value.trim().to_string(),
        })
    }
}

fn systemd_failed_target(
    target_id: String,
    write_attempted: bool,
    pipeline_stage: PipelineStage,
    error: &io::Error,
) -> TargetOutcome {
    TargetOutcome {
        target_id,
        pipeline_stage,
        support: SupportState::Supported,
        reason: OutcomeReasonCode::WriteFailed,
        write_attempted,
        write_outcome: WriteOutcome::Failed {
            error_kind: ErrorKindCode::from_io(error),
        },
        readback: ReadbackOutcome::Unavailable,
        ownership: OwnershipState::Unknown,
        pending_restore: RestoreState::NotEvaluated,
        responsible_subsystem: ResponsibleSubsystem::Systemd,
        detail: Some(format!("systemd property operation failed: {:?}", error.kind())),
    }
}

fn failed_restore(target_id: &str, attempted: bool, error: &io::Error) -> RestoreOutcome {
    RestoreOutcome {
        target_id: target_id.to_string(),
        pipeline_stage: PipelineStage::Restore,
        reason: OutcomeReasonCode::RestoreFailed,
        write_attempted: attempted,
        write_outcome: WriteOutcome::RestorationFailed {
            error_kind: ErrorKindCode::from_io(error),
        },
        readback: ReadbackOutcome::NotPerformed,
        ownership: OwnershipState::Optid,
        pending_restore: RestoreState::Pending,
        responsible_subsystem: ResponsibleSubsystem::Restoration,
        detail: Some(format!("restore failed: {:?}", error.kind())),
    }
}

fn relinquished_outcome(target_id: &str, detail: &str) -> RestoreOutcome {
    RestoreOutcome {
        target_id: target_id.to_string(),
        pipeline_stage: PipelineStage::Restore,
        reason: OutcomeReasonCode::OwnershipRelinquished,
        write_attempted: false,
        write_outcome: WriteOutcome::OwnershipRelinquished,
        readback: ReadbackOutcome::Unavailable,
        ownership: OwnershipState::Relinquished,
        pending_restore: RestoreState::NotApplicable,
        responsible_subsystem: ResponsibleSubsystem::Restoration,
        detail: Some(detail.to_string()),
    }
}

fn legacy_restore_supported(key: &str) -> bool {
    ["rpm_", "dev_", "aspm_", "alpm_", "bl_"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

fn sanitize_identity(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
