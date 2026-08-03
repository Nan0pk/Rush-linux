const TRANSACTION_SCHEMA_VERSION: u32 = 1;
#[cfg(not(test))]
const DEFAULT_RECOVERY_DIR: &str = "/var/lib/optid/recovery";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionOperation {
    CpuEpp,
    PlatformProfile,
    SystemdProperty,
    VmSysctl,
    CpuDmaPmQos,
    DevicePmQos,
    RuntimePm,
    PcieAspm,
    SataAlpm,
    Backlight,
}

impl TransactionOperation {
    fn from_action(action: &Action) -> Self {
        match action {
            Action::CpuEpp { .. } => Self::CpuEpp,
            Action::PlatformProfile { .. } => Self::PlatformProfile,
            Action::SystemdSetProperty { .. } => Self::SystemdProperty,
            Action::VmSysctl { .. } => Self::VmSysctl,
            Action::CpuDmaLatency { .. } => Self::CpuDmaPmQos,
            Action::DeviceResumeLatency { .. } => Self::DevicePmQos,
            Action::RuntimePm { .. } => Self::RuntimePm,
            Action::PcieAspm { .. } => Self::PcieAspm,
            Action::SataAlpm { .. } => Self::SataAlpm,
            Action::Backlight { .. } => Self::Backlight,
        }
    }

    fn rollback_method(self) -> &'static str {
        match self {
            Self::CpuEpp => "restore each captured CPU-policy startup value",
            Self::PlatformProfile => "restore the captured advertised platform profile",
            Self::SystemdProperty => "restore the captured explicit property or remove the runtime override",
            Self::VmSysctl => "restore every captured boot/startup VM sysctl value",
            Self::CpuDmaPmQos => "close the optid-owned request descriptor",
            Self::DevicePmQos => "restore the captured request or remove only the optid-owned request",
            Self::RuntimePm => "restore captured power/control and autosuspend delay",
            Self::PcieAspm => "restore the captured PCIe link state",
            Self::SataAlpm => "restore the captured SCSI-host policy",
            Self::Backlight => "restore the captured user-owned raw brightness",
        }
    }

    fn stabilization_method(self) -> &'static str {
        match self {
            Self::CpuEpp => "use an advertised balanced preference",
            Self::PlatformProfile => "use balanced only when advertised",
            Self::SystemdProperty => "relinquish the runtime override to systemd",
            Self::VmSysctl => "use recorded distribution boot policy with provenance",
            Self::CpuDmaPmQos => "close all optid-owned descriptors and quarantine",
            Self::DevicePmQos => "relax the owned request or force the device active",
            Self::RuntimePm => "force the device active with power/control=on",
            Self::PcieAspm => "disable only the optid-enabled deeper state",
            Self::SataAlpm => "use max_performance when supported",
            Self::Backlight => "use a hardware-verified visibility floor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Prepared,
    Committed,
    Compensating,
    Compensated,
    Relinquished,
}

impl TransactionPhase {
    #[cfg(test)]
    fn is_terminal(self) -> bool {
        matches!(self, Self::Compensated | Self::Relinquished)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TransactionRecord {
    schema_version: u32,
    generation: String,
    owner: String,
    domain: String,
    operation: TransactionOperation,
    target_id: String,
    canonical_identity: String,
    target: TargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_journal_key: Option<String>,
    original: StoredValue,
    intended: StoredValue,
    rollback_method: String,
    stabilization_method: String,
    phase: TransactionPhase,
    created_at_unix: u64,
    updated_at_unix: u64,
}

#[derive(Debug, Clone)]
struct TransactionHandle {
    path: PathBuf,
    prepared: TransactionRecord,
    previous: Option<TransactionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompensationDisposition {
    Restored,
    AlreadyRestored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionErrorKind {
    JournalIo,
    InvalidRecord,
    StaleGeneration,
    IdentityMismatch,
    PhaseConflict,
    ReadbackMismatch,
    CompensationFailed,
}

#[derive(Debug)]
struct TransactionError {
    kind: TransactionErrorKind,
    detail: String,
}

impl TransactionError {
    fn new(kind: TransactionErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    fn io(stage: &str, error: io::Error) -> Self {
        Self::new(
            TransactionErrorKind::JournalIo,
            format!("{stage}: {:?}: {error}", error.kind()),
        )
    }
}

impl std::fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for TransactionError {}

impl From<TransactionError> for io::Error {
    fn from(error: TransactionError) -> Self {
        io::Error::other(error.to_string())
    }
}

#[derive(Debug, Clone)]
struct TransactionEngine {
    root: PathBuf,
    generation: String,
    owner: String,
}

impl TransactionEngine {
    fn for_process(root: PathBuf) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        Self::new(
            root,
            format!("{nanos:032x}-{:08x}", std::process::id()),
        )
    }

    fn new(root: PathBuf, generation: String) -> Self {
        Self {
            root,
            generation,
            owner: "optid".to_string(),
        }
    }

    fn record_path(&self, target_id: &str) -> PathBuf {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        target_id.hash(&mut hasher);
        let readable = sanitize_identity(target_id);
        self.root
            .join(format!("{readable}-{:016x}.json", hasher.finish()))
    }

    fn temp_path(&self, final_path: &Path) -> PathBuf {
        let file_name = final_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("transaction.json");
        final_path.with_file_name(format!(
            ".{file_name}.{}.tmp",
            sanitize_identity(&self.generation)
        ))
    }

    fn canonical_identity(
        &self,
        io: &dyn KernelIo,
        target: &TargetKind,
    ) -> Result<String, TransactionError> {
        let canonical = |path: &Path| {
            io.canonicalize(path)
                .map(|value| value.display().to_string())
                .map_err(|error| TransactionError::io("canonicalize transaction target", error))
        };
        match target {
            TargetKind::KernelValue { path } => {
                canonical(path).map(|path| format!("kernel:{path}"))
            }
            TargetKind::PmqosCpu => Ok("pmqos:/dev/cpu_dma_latency".to_string()),
            TargetKind::PmqosDevice { path } => {
                canonical(path).map(|path| format!("device-pmqos:{path}"))
            }
            TargetKind::RuntimePm {
                control_path,
                delay_path,
            } => {
                let control = canonical(control_path)?;
                let delay = delay_path
                    .as_ref()
                    .map(|path| canonical(path))
                    .transpose()?
                    .unwrap_or_else(|| "absent".to_string());
                Ok(format!("runtime-pm:control={control};delay={delay}"))
            }
            TargetKind::SystemdProperty { unit, property } => {
                Ok(format!("systemd:{unit}:{property}"))
            }
        }
    }

    fn load_record(
        &self,
        io: &dyn KernelIo,
        path: &Path,
    ) -> Result<TransactionRecord, TransactionError> {
        let content = io
            .read_to_string(path)
            .map_err(|error| TransactionError::io("read transaction record", error))?;
        let record: TransactionRecord = serde_json::from_str(&content).map_err(|error| {
            TransactionError::new(
                TransactionErrorKind::InvalidRecord,
                format!("parse {}: {error}", path.display()),
            )
        })?;
        if record.schema_version != TRANSACTION_SCHEMA_VERSION {
            return Err(TransactionError::new(
                TransactionErrorKind::InvalidRecord,
                format!(
                    "unsupported transaction schema {} in {}",
                    record.schema_version,
                    path.display()
                ),
            ));
        }
        Ok(record)
    }

    fn durable_store(
        &self,
        io: &dyn KernelIo,
        record: &TransactionRecord,
    ) -> Result<PathBuf, TransactionError> {
        let final_path = self.record_path(&record.target_id);
        let temp_path = self.temp_path(&final_path);
        let content = serde_json::to_string_pretty(record).map_err(|error| {
            TransactionError::new(
                TransactionErrorKind::InvalidRecord,
                format!("serialize transaction {}: {error}", record.target_id),
            )
        })?;

        io.create_dir_all(&self.root)
            .map_err(|error| TransactionError::io("create recovery directory", error))?;
        io.write_state_file(&temp_path, &content)
            .map_err(|error| TransactionError::io("write transaction temp file", error))?;
        io.sync_file(&temp_path)
            .map_err(|error| TransactionError::io("fsync transaction temp file", error))?;
        io.rename(&temp_path, &final_path)
            .map_err(|error| TransactionError::io("publish transaction record", error))?;
        io.sync_dir(&self.root)
            .map_err(|error| TransactionError::io("fsync recovery directory", error))?;
        Ok(final_path)
    }

    fn prepare(
        &self,
        io: &dyn KernelIo,
        action: &Action,
        desired: &DesiredTarget,
        original: &StoredValue,
    ) -> Result<TransactionHandle, TransactionError> {
        let operation = TransactionOperation::from_action(action);
        let canonical_identity = self.canonical_identity(io, &desired.target)?;
        let path = self.record_path(&desired.target_id);

        if io.exists(&path) {
            let existing = self.load_record(io, &path)?;
            if existing.generation != self.generation {
                return Err(TransactionError::new(
                    TransactionErrorKind::StaleGeneration,
                    format!(
                        "{} is owned by generation {}, current generation is {}",
                        desired.target_id, existing.generation, self.generation
                    ),
                ));
            }
            if existing.canonical_identity != canonical_identity {
                return Err(TransactionError::new(
                    TransactionErrorKind::IdentityMismatch,
                    format!(
                        "{} identity changed from {} to {}",
                        desired.target_id, existing.canonical_identity, canonical_identity
                    ),
                ));
            }
            if existing.original != *original {
                return Err(TransactionError::new(
                    TransactionErrorKind::PhaseConflict,
                    format!(
                        "{} original changed while transaction is unresolved",
                        desired.target_id
                    ),
                ));
            }
            if existing.phase == TransactionPhase::Prepared
                && existing.intended == desired.desired
            {
                return Ok(TransactionHandle {
                    path,
                    prepared: existing.clone(),
                    previous: Some(existing),
                });
            }
            if existing.phase == TransactionPhase::Committed {
                let previous = existing.clone();
                let mut refreshed = existing;
                refreshed.intended = desired.desired.clone();
                refreshed.legacy_journal_key = desired.legacy_journal_key.clone();
                refreshed.phase = TransactionPhase::Prepared;
                refreshed.updated_at_unix = io.now_unix();
                self.durable_store(io, &refreshed)?;
                return Ok(TransactionHandle {
                    path,
                    prepared: refreshed,
                    previous: Some(previous),
                });
            }
            return Err(TransactionError::new(
                TransactionErrorKind::PhaseConflict,
                format!(
                    "{} already has an unresolved {:?} transaction",
                    desired.target_id, existing.phase
                ),
            ));
        }

        let now = io.now_unix();
        let record = TransactionRecord {
            schema_version: TRANSACTION_SCHEMA_VERSION,
            generation: self.generation.clone(),
            owner: self.owner.clone(),
            domain: desired.domain.clone(),
            operation,
            target_id: desired.target_id.clone(),
            canonical_identity,
            target: desired.target.clone(),
            legacy_journal_key: desired.legacy_journal_key.clone(),
            original: original.clone(),
            intended: desired.desired.clone(),
            rollback_method: operation.rollback_method().to_string(),
            stabilization_method: operation.stabilization_method().to_string(),
            phase: TransactionPhase::Prepared,
            created_at_unix: now,
            updated_at_unix: now,
        };
        let path = self.durable_store(io, &record)?;
        Ok(TransactionHandle {
            path,
            prepared: record,
            previous: None,
        })
    }

    fn abort_prepare(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
    ) -> Result<(), TransactionError> {
        match &handle.previous {
            Some(previous) => {
                self.durable_store(io, previous)?;
                Ok(())
            }
            None => self.compact(io, handle),
        }
    }

    fn current_record(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
    ) -> Result<TransactionRecord, TransactionError> {
        match self.load_record(io, &handle.path) {
            Ok(record) => Ok(record),
            Err(error)
                if error.kind == TransactionErrorKind::JournalIo
                    && !io.exists(&handle.path) =>
            {
                Ok(handle.prepared.clone())
            }
            Err(error) => Err(error),
        }
    }

    fn validate_generation_and_identity(
        &self,
        io: &dyn KernelIo,
        record: &TransactionRecord,
    ) -> Result<(), TransactionError> {
        if record.generation != self.generation {
            return Err(TransactionError::new(
                TransactionErrorKind::StaleGeneration,
                format!(
                    "{} belongs to generation {}, not {}",
                    record.target_id, record.generation, self.generation
                ),
            ));
        }
        let current = self.canonical_identity(io, &record.target)?;
        if current != record.canonical_identity {
            return Err(TransactionError::new(
                TransactionErrorKind::IdentityMismatch,
                format!(
                    "{} identity changed from {} to {}",
                    record.target_id, record.canonical_identity, current
                ),
            ));
        }
        Ok(())
    }

    fn transition(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
        allowed: &[TransactionPhase],
        phase: TransactionPhase,
    ) -> Result<TransactionRecord, TransactionError> {
        let mut record = self.load_record(io, &handle.path)?;
        self.validate_generation_and_identity(io, &record)?;
        if record.phase == phase {
            return Ok(record);
        }
        if !allowed.contains(&record.phase) {
            return Err(TransactionError::new(
                TransactionErrorKind::PhaseConflict,
                format!(
                    "{} cannot transition from {:?} to {:?}",
                    record.target_id, record.phase, phase
                ),
            ));
        }
        record.phase = phase;
        record.updated_at_unix = io.now_unix();
        self.durable_store(io, &record)?;
        Ok(record)
    }

    fn commit(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
    ) -> Result<(), TransactionError> {
        self.transition(
            io,
            handle,
            &[TransactionPhase::Prepared, TransactionPhase::Committed],
            TransactionPhase::Committed,
        )?;
        Ok(())
    }

    fn begin_compensation(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
    ) -> Result<TransactionRecord, TransactionError> {
        self.transition(
            io,
            handle,
            &[
                TransactionPhase::Prepared,
                TransactionPhase::Committed,
                TransactionPhase::Compensating,
            ],
            TransactionPhase::Compensating,
        )
    }

    fn mark_compensated(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
    ) -> Result<(), TransactionError> {
        self.transition(
            io,
            handle,
            &[
                TransactionPhase::Prepared,
                TransactionPhase::Committed,
                TransactionPhase::Compensating,
                TransactionPhase::Compensated,
            ],
            TransactionPhase::Compensated,
        )?;
        Ok(())
    }

    fn mark_terminal_without_generation_check(
        &self,
        io: &dyn KernelIo,
        path: &Path,
        phase: TransactionPhase,
    ) -> Result<(), TransactionError> {
        let mut record = self.load_record(io, path)?;
        if record.phase == phase {
            return Ok(());
        }
        record.phase = phase;
        record.updated_at_unix = io.now_unix();
        self.durable_store(io, &record)?;
        Ok(())
    }

    fn compact_path(&self, io: &dyn KernelIo, path: &Path) -> Result<(), TransactionError> {
        match io.remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(TransactionError::io(
                    "remove completed transaction record",
                    error,
                ))
            }
        }
        if io.exists(&self.root) {
            io.sync_dir(&self.root)
                .map_err(|error| TransactionError::io("fsync recovery directory", error))?;
        }
        Ok(())
    }

    fn compact(
        &self,
        io: &dyn KernelIo,
        handle: &TransactionHandle,
    ) -> Result<(), TransactionError> {
        self.compact_path(io, &handle.path)
    }

    fn finish_handback(
        &self,
        io: &dyn KernelIo,
        target_id: &str,
        relinquished: bool,
    ) -> Result<(), TransactionError> {
        let path = self.record_path(target_id);
        if !io.exists(&path) {
            return Ok(());
        }
        self.mark_terminal_without_generation_check(
            io,
            &path,
            if relinquished {
                TransactionPhase::Relinquished
            } else {
                TransactionPhase::Compensated
            },
        )?;
        self.compact_path(io, &path)
    }

    #[cfg(test)]
    fn active_records(
        &self,
        io: &dyn KernelIo,
    ) -> Result<Vec<TransactionRecord>, TransactionError> {
        let entries = match io.read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(TransactionError::io("list recovery directory", error)),
        };
        let mut records = Vec::new();
        for path in entries {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let record = self.load_record(io, &path)?;
            if !record.phase.is_terminal() {
                records.push(record);
            }
        }
        records.sort_by(|left, right| left.target_id.cmp(&right.target_id));
        Ok(records)
    }
}

fn default_recovery_dir(state_dir: &Path) -> PathBuf {
    #[cfg(test)]
    {
        state_dir.join("s2d-recovery")
    }
    #[cfg(not(test))]
    {
        let _ = state_dir;
        PathBuf::from(DEFAULT_RECOVERY_DIR)
    }
}

impl Reconciler {
    fn prepare_transactions(
        &self,
        actuator: &mut Actuator,
        action: &Action,
        targets: &[DesiredTarget],
    ) -> Result<BTreeMap<String, TransactionHandle>, TransactionError> {
        let mut prepared = BTreeMap::new();
        for desired in targets {
            let original = self
                .targets
                .get(&desired.target_id)
                .and_then(|state| state.baseline.as_ref())
                .ok_or_else(|| {
                    TransactionError::new(
                        TransactionErrorKind::InvalidRecord,
                        format!("{} has no captured baseline", desired.target_id),
                    )
                })?;
            match self
                .transactions
                .prepare(actuator.kernel.as_ref(), action, desired, original)
            {
                Ok(handle) => {
                    prepared.insert(desired.target_id.clone(), handle);
                }
                Err(error) => {
                    for handle in prepared.values() {
                        let _ = self
                            .transactions
                            .abort_prepare(actuator.kernel.as_ref(), handle);
                    }
                    return Err(error);
                }
            }
        }
        Ok(prepared)
    }

    fn compensation_for_handle(
        &self,
        actuator: &mut Actuator,
        handle: &TransactionHandle,
    ) -> Result<CompensationDisposition, TransactionError> {
        let record = self.transactions.current_record(actuator.kernel.as_ref(), handle)?;
        self.transactions
            .validate_generation_and_identity(actuator.kernel.as_ref(), &record)?;

        let current = self.read_target(actuator, &record.target).map_err(|error| {
            TransactionError::new(
                TransactionErrorKind::CompensationFailed,
                format!("read {} before compensation: {error}", record.target_id),
            )
        })?;
        if current == record.original {
            if let Some(key) = &record.legacy_journal_key {
                clear_journal_with(actuator.kernel.as_ref(), &self.state_dir, key);
            }
            if actuator.kernel.exists(&handle.path) {
                self.transactions
                    .mark_compensated(actuator.kernel.as_ref(), handle)?;
                self.transactions
                    .compact(actuator.kernel.as_ref(), handle)?;
            }
            return Ok(CompensationDisposition::AlreadyRestored);
        }

        // A failed write can yield a kernel-normalized value that is neither
        // byte-equal to the intended nor original value. Canonical identity is
        // the hard path-reuse guard; compensate the same object rather than
        // leave a known failed mutation active.

        self.transactions
            .begin_compensation(actuator.kernel.as_ref(), handle)?;
        write_target_for_restore(
            actuator,
            self.systemd.as_ref(),
            &record.target,
            &record.original,
        )
        .map_err(|error| {
            TransactionError::new(
                TransactionErrorKind::CompensationFailed,
                format!("write {} during compensation: {error}", record.target_id),
            )
        })?;
        let readback = self.read_target(actuator, &record.target).map_err(|error| {
            TransactionError::new(
                TransactionErrorKind::CompensationFailed,
                format!("read {} after compensation: {error}", record.target_id),
            )
        })?;
        if readback != record.original {
            return Err(TransactionError::new(
                TransactionErrorKind::ReadbackMismatch,
                format!(
                    "{} compensation expected {}, observed {}",
                    record.target_id,
                    record.original.public_value(),
                    readback.public_value()
                ),
            ));
        }

        if let Some(key) = &record.legacy_journal_key {
            clear_journal_with(actuator.kernel.as_ref(), &self.state_dir, key);
        }
        self.transactions
            .mark_compensated(actuator.kernel.as_ref(), handle)?;
        self.transactions
            .compact(actuator.kernel.as_ref(), handle)?;
        Ok(CompensationDisposition::Restored)
    }

    fn compensate_all(
        &self,
        actuator: &mut Actuator,
        handles: &BTreeMap<String, TransactionHandle>,
    ) -> Result<(), TransactionError> {
        for handle in handles.values() {
            self.compensation_for_handle(actuator, handle)?;
        }
        Ok(())
    }

    fn finalize_transactions(
        &self,
        actuator: &mut Actuator,
        targets: &[DesiredTarget],
        handles: &BTreeMap<String, TransactionHandle>,
        outcome: &mut ActionOutcome,
    ) -> Result<(), TransactionError> {
        for desired in targets {
            let Some(handle) = handles.get(&desired.target_id) else {
                continue;
            };
            let index = outcome
                .targets
                .iter()
                .position(|target| target.target_id == desired.target_id)
                .or_else(|| (outcome.targets.len() == 1).then_some(0));

            let Some(index) = index else {
                self.compensation_for_handle(actuator, handle)?;
                return Err(TransactionError::new(
                    TransactionErrorKind::InvalidRecord,
                    format!("{} produced no target outcome", desired.target_id),
                ));
            };
            let write_attempted = outcome.targets[index].write_attempted;
            let confirmed = matches!(
                outcome.targets[index].readback,
                ReadbackOutcome::Confirmed { .. }
            ) && outcome.targets[index].ownership == OwnershipState::Optid;

            if !write_attempted {
                let retained_ownership = outcome.targets[index].ownership == OwnershipState::Optid
                    && outcome.targets[index].pending_restore == RestoreState::Pending
                    && matches!(
                        outcome.targets[index].readback,
                        ReadbackOutcome::Confirmed { .. }
                    );
                if retained_ownership {
                    self.transactions
                        .commit(actuator.kernel.as_ref(), handle)?;
                } else if outcome.targets[index].ownership == OwnershipState::Relinquished {
                    self.transactions.finish_handback(
                        actuator.kernel.as_ref(),
                        &desired.target_id,
                        true,
                    )?;
                } else {
                    self.transactions
                        .abort_prepare(actuator.kernel.as_ref(), handle)?;
                }
                continue;
            }
            if confirmed {
                self.transactions
                    .commit(actuator.kernel.as_ref(), handle)?;
                continue;
            }

            let disposition = self.compensation_for_handle(actuator, handle)?;
            let detail = match disposition {
                CompensationDisposition::Restored => {
                    "S2D compensation restored and verified the captured original"
                }
                CompensationDisposition::AlreadyRestored => {
                    "S2D compensation found the captured original already restored"
                }
            };
            let target = &mut outcome.targets[index];
            target.ownership = OwnershipState::Unowned;
            target.pending_restore = RestoreState::Restored;
            target.detail = Some(match target.detail.take() {
                Some(existing) => format!("{existing}; {detail}"),
                None => detail.to_string(),
            });
        }
        Ok(())
    }
}
