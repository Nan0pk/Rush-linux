//! S1D typed per-lever handback and semantic-envelope contracts.
//!
//! The accepted D2 fail-passive architecture requires every current or planned
//! hardware lever to answer four questions before later safety packages migrate
//! it to sealed actuation:
//!
//! 1. What stable object and ABI does optid operate on?
//! 2. Which values are inside the frozen semantic envelope?
//! 3. How is the exact captured original restored without overwriting drift?
//! 4. Which separately named conservative state may be used only when exact
//!    rollback is impossible, and how is either result verified?
//!
//! This module freezes those answers. It intentionally does **not** connect new
//! writes to the running daemon; S2D and S4D consume these contracts when they
//! add durable transactions and sealed typed capabilities. Current production
//! defaults therefore remain unchanged.

use std::collections::BTreeSet;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lever {
    CpuDmaPmQos,
    DevicePmQos,
    CpuEpp,
    PlatformProfile,
    RuntimePm,
    SataAlpm,
    PcieAspm,
    Backlight,
    VmSysctl,
    PowercapPl1,
    DgpuRuntimePm,
}

impl Lever {
    pub const ALL: [Self; 11] = [
        Self::CpuDmaPmQos,
        Self::DevicePmQos,
        Self::CpuEpp,
        Self::PlatformProfile,
        Self::RuntimePm,
        Self::SataAlpm,
        Self::PcieAspm,
        Self::Backlight,
        Self::VmSysctl,
        Self::PowercapPl1,
        Self::DgpuRuntimePm,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CpuDmaPmQos => "cpu_dma_pm_qos",
            Self::DevicePmQos => "device_pm_qos",
            Self::CpuEpp => "cpu_epp",
            Self::PlatformProfile => "platform_profile",
            Self::RuntimePm => "runtime_pm",
            Self::SataAlpm => "sata_alpm",
            Self::PcieAspm => "pcie_aspm",
            Self::Backlight => "backlight",
            Self::VmSysctl => "vm_sysctl",
            Self::PowercapPl1 => "powercap_pl1",
            Self::DgpuRuntimePm => "dgpu_runtime_pm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationState {
    Current,
    Planned,
}

impl ImplementationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Planned => "planned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StableIdentity {
    OptidDescriptorOwner,
    DeviceHwidFirmware,
    CpuPolicy,
    PlatformFirmware,
    ScsiHostController,
    PciFunctionTopology,
    DisplayPanel,
    BootPolicy,
    CpuPackageFirmware,
    GpuDriverFirmware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportedAbi {
    CpuDmaLatencyFd,
    DeviceResumeLatencyUs,
    CpuFreqEpp,
    AcpiPlatformProfile,
    RuntimePmControlDelay,
    SataLinkPowerPolicy,
    PcieL1Aspm,
    BacklightBrightness,
    ProcVmSysctl,
    PowercapConstraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginalCapture {
    DescriptorOwnership,
    StartupValue,
    ControlAndDelay,
    UserOwnedRawValue,
    BootSnapshot,
    FirmwareStartupLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipRule {
    DescriptorOwned,
    CompareLastConfirmedRelinquishOnDrift,
    UserOwnedCompareLastConfirmed,
    TransactionMemberCompareAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackMethod {
    CloseOwnedDescriptor,
    RestoreCapturedRequestOrRemoveOwnedRequest,
    RestoreCapturedStartupValue,
    RestoreCapturedControlAndDelay,
    RestoreCapturedHostPolicy,
    RestoreCapturedLinkState,
    RestoreCapturedUserBrightness,
    RestoreCapturedBootValues,
    RestoreCapturedFirmwareLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StabilizationMethod {
    CloseAllOwnedDescriptorsAndQuarantine,
    RelaxOwnedRequestOrForceDeviceActive,
    AdvertisedBalancedPreference,
    AdvertisedBalancedProfile,
    ForceDeviceActive,
    MaxPerformanceIfSupported,
    DisableOptidEnabledDeeperState,
    HardwareVerifiedVisibilityFloor,
    RecordedDistributionBootPolicy,
    StopWritingOrReviewedConservativeCap,
}

impl StabilizationMethod {
    pub const fn requires_external_evidence(self) -> bool {
        matches!(
            self,
            Self::AdvertisedBalancedPreference
                | Self::AdvertisedBalancedProfile
                | Self::MaxPerformanceIfSupported
                | Self::HardwareVerifiedVisibilityFloor
                | Self::RecordedDistributionBootPolicy
                | Self::StopWritingOrReviewedConservativeCap
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    DescriptorOwnershipAndEffectiveConstraint,
    ReadbackAndDeviceIdentity,
    ReadEveryCpuPolicy,
    ReadbackAndAdvertisedChoice,
    ReadbackAndScsiHostIdentity,
    ReadbackAndPciTopology,
    ReadbackAndPanelIdentity,
    ReadbackEveryTransactionMember,
    BoundsReadbackPackageIdentityTelemetry,
    ReadbackGpuDriverFirmwareIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedBehavior {
    DenyActuationAllowObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SysctlRule {
    pub name: &'static str,
    pub min: i64,
    pub max: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticEnvelope {
    Numeric {
        min: i64,
        max: i64,
        allow_release: bool,
    },
    Tokens {
        allowed: &'static [&'static str],
    },
    RuntimePm {
        min_delay_ms: i32,
        max_delay_ms: i32,
    },
    Boolean,
    BacklightPercent {
        min: u8,
        max: u8,
        requires_hardware_floor: bool,
    },
    VmSysctls {
        rules: &'static [SysctlRule],
    },
    PowercapPercent {
        min: u8,
        max: u8,
        requires_reviewed_cap: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeValue<'a> {
    Release,
    Integer(i64),
    Token(&'a str),
    RuntimePm { control: &'a str, delay_ms: i32 },
    Boolean(bool),
    BacklightPercent(u8),
    VmSysctl { name: &'a str, value: i64 },
    PowercapPercent(u8),
}

impl SemanticEnvelope {
    pub fn permits(self, value: EnvelopeValue<'_>) -> bool {
        match (self, value) {
            (
                Self::Numeric {
                    allow_release: true,
                    ..
                },
                EnvelopeValue::Release,
            ) => true,
            (Self::Numeric { min, max, .. }, EnvelopeValue::Integer(value)) => {
                (min..=max).contains(&value)
            }
            (Self::Tokens { allowed }, EnvelopeValue::Token(value)) => allowed.contains(&value),
            (
                Self::RuntimePm {
                    min_delay_ms,
                    max_delay_ms,
                },
                EnvelopeValue::RuntimePm { control, delay_ms },
            ) => control == "auto" && (min_delay_ms..=max_delay_ms).contains(&delay_ms),
            (Self::Boolean, EnvelopeValue::Boolean(_)) => true,
            (Self::BacklightPercent { min, max, .. }, EnvelopeValue::BacklightPercent(value)) => {
                (min..=max).contains(&value)
            }
            (Self::VmSysctls { rules }, EnvelopeValue::VmSysctl { name, value }) => rules
                .iter()
                .find(|rule| rule.name == name)
                .is_some_and(|rule| (rule.min..=rule.max).contains(&value)),
            (Self::PowercapPercent { min, max, .. }, EnvelopeValue::PowercapPercent(value)) => {
                (min..=max).contains(&value)
            }
            _ => false,
        }
    }

    pub fn is_tightening_of(self, broader: Self) -> bool {
        match (self, broader) {
            (
                Self::Numeric {
                    min,
                    max,
                    allow_release,
                },
                Self::Numeric {
                    min: broader_min,
                    max: broader_max,
                    allow_release: broader_release,
                },
            ) => min >= broader_min && max <= broader_max && (!allow_release || broader_release),
            (Self::Tokens { allowed }, Self::Tokens { allowed: broader }) => {
                allowed.iter().all(|value| broader.contains(value))
            }
            (
                Self::RuntimePm {
                    min_delay_ms,
                    max_delay_ms,
                },
                Self::RuntimePm {
                    min_delay_ms: broader_min,
                    max_delay_ms: broader_max,
                },
            ) => min_delay_ms >= broader_min && max_delay_ms <= broader_max,
            (Self::Boolean, Self::Boolean) => true,
            (
                Self::BacklightPercent {
                    min,
                    max,
                    requires_hardware_floor,
                },
                Self::BacklightPercent {
                    min: broader_min,
                    max: broader_max,
                    requires_hardware_floor: broader_requires_floor,
                },
            ) => {
                min >= broader_min
                    && max <= broader_max
                    && (requires_hardware_floor || !broader_requires_floor)
            }
            (Self::VmSysctls { rules }, Self::VmSysctls { rules: broader }) => {
                rules.iter().all(|rule| {
                    broader.iter().any(|candidate| {
                        candidate.name == rule.name
                            && rule.min >= candidate.min
                            && rule.max <= candidate.max
                    })
                })
            }
            (
                Self::PowercapPercent {
                    min,
                    max,
                    requires_reviewed_cap,
                },
                Self::PowercapPercent {
                    min: broader_min,
                    max: broader_max,
                    requires_reviewed_cap: broader_requires_cap,
                },
            ) => {
                min >= broader_min
                    && max <= broader_max
                    && (requires_reviewed_cap || !broader_requires_cap)
            }
            _ => false,
        }
    }

    fn representative_value(self) -> Option<EnvelopeValue<'static>> {
        match self {
            Self::Numeric {
                min, allow_release, ..
            } => Some(if allow_release {
                EnvelopeValue::Release
            } else {
                EnvelopeValue::Integer(min)
            }),
            Self::Tokens { allowed } => allowed.first().copied().map(EnvelopeValue::Token),
            Self::RuntimePm { min_delay_ms, .. } => Some(EnvelopeValue::RuntimePm {
                control: "auto",
                delay_ms: min_delay_ms,
            }),
            Self::Boolean => Some(EnvelopeValue::Boolean(false)),
            Self::BacklightPercent { min, .. } => Some(EnvelopeValue::BacklightPercent(min)),
            Self::VmSysctls { rules } => rules.first().map(|rule| EnvelopeValue::VmSysctl {
                name: rule.name,
                value: rule.min,
            }),
            Self::PowercapPercent { min, .. } => Some(EnvelopeValue::PowercapPercent(min)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LeverContract {
    pub lever: Lever,
    pub implementation: ImplementationState,
    pub stable_identity: StableIdentity,
    pub supported_abi: SupportedAbi,
    pub credible_worst_case: &'static str,
    pub semantic_envelope: SemanticEnvelope,
    pub original_capture: OriginalCapture,
    pub ownership_rule: OwnershipRule,
    pub rollback: RollbackMethod,
    pub stabilization: StabilizationMethod,
    pub verification: VerificationMethod,
    pub unsupported: UnsupportedBehavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandbackClassification {
    Restored,
    Stabilized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandbackPlan {
    pub lever: Lever,
    pub classification: HandbackClassification,
    pub rollback: Option<RollbackMethod>,
    pub stabilization: Option<StabilizationMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandbackError {
    OriginalMissing,
    OriginalUnverified,
    StabilizationEvidenceMissing,
}

impl LeverContract {
    pub fn rollback_plan(
        self,
        original_present: bool,
        original_verified: bool,
    ) -> Result<HandbackPlan, HandbackError> {
        if !original_present {
            return Err(HandbackError::OriginalMissing);
        }
        if !original_verified {
            return Err(HandbackError::OriginalUnverified);
        }
        Ok(HandbackPlan {
            lever: self.lever,
            classification: HandbackClassification::Restored,
            rollback: Some(self.rollback),
            stabilization: None,
        })
    }

    pub fn stabilization_plan(
        self,
        supporting_evidence_present: bool,
    ) -> Result<HandbackPlan, HandbackError> {
        if self.stabilization.requires_external_evidence() && !supporting_evidence_present {
            return Err(HandbackError::StabilizationEvidenceMissing);
        }
        Ok(HandbackPlan {
            lever: self.lever,
            classification: HandbackClassification::Stabilized,
            rollback: None,
            stabilization: Some(self.stabilization),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActuationEvidence<'a> {
    pub stable_identity_verified: bool,
    pub original_value_captured: bool,
    pub verification_available: bool,
    pub value: Option<EnvelopeValue<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationDenial {
    RegistryInvalid,
    LeverNotImplemented,
    StableIdentityMissing,
    OriginalValueMissing,
    EnvelopeValueMissing,
    ValueOutsideEnvelope,
    VerificationUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationDecision {
    Allowed,
    Denied(AuthorizationDenial),
}

pub fn authorize(lever: Lever, evidence: ActuationEvidence<'_>) -> AuthorizationDecision {
    if validate_registry().is_err() {
        return AuthorizationDecision::Denied(AuthorizationDenial::RegistryInvalid);
    }
    let contract = contract_for(lever);
    if contract.implementation != ImplementationState::Current {
        return AuthorizationDecision::Denied(AuthorizationDenial::LeverNotImplemented);
    }
    if !evidence.stable_identity_verified {
        return AuthorizationDecision::Denied(AuthorizationDenial::StableIdentityMissing);
    }
    if !evidence.original_value_captured {
        return AuthorizationDecision::Denied(AuthorizationDenial::OriginalValueMissing);
    }
    let Some(value) = evidence.value else {
        return AuthorizationDecision::Denied(AuthorizationDenial::EnvelopeValueMissing);
    };
    if !contract.semantic_envelope.permits(value) {
        return AuthorizationDecision::Denied(AuthorizationDenial::ValueOutsideEnvelope);
    }
    if !evidence.verification_available {
        return AuthorizationDecision::Denied(AuthorizationDenial::VerificationUnavailable);
    }
    AuthorizationDecision::Allowed
}

const EPP_VALUES: &[&str] = &[
    "default",
    "performance",
    "balance_performance",
    "balance_power",
    "power",
];
const PLATFORM_PROFILES: &[&str] = &[
    "balanced",
    "performance",
    "low-power",
    "cool",
    "quiet",
    "balanced-performance",
];
const SATA_POLICIES: &[&str] = &[
    "max_performance",
    "medium_power",
    "med_power_with_dipm",
    "min_power",
];
const VM_SYSCTLS: &[SysctlRule] = &[
    SysctlRule {
        name: "swappiness",
        min: 0,
        max: 200,
    },
    SysctlRule {
        name: "dirty_background_bytes",
        min: 0,
        max: i64::MAX,
    },
    SysctlRule {
        name: "dirty_bytes",
        min: 0,
        max: i64::MAX,
    },
];

pub const CONTRACTS: [LeverContract; 11] = [
    LeverContract {
        lever: Lever::CpuDmaPmQos,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::OptidDescriptorOwner,
        supported_abi: SupportedAbi::CpuDmaLatencyFd,
        credible_worst_case:
            "latency request causes severe power loss or is leaked after optid exits",
        semantic_envelope: SemanticEnvelope::Numeric {
            min: 0,
            max: i32::MAX as i64,
            allow_release: true,
        },
        original_capture: OriginalCapture::DescriptorOwnership,
        ownership_rule: OwnershipRule::DescriptorOwned,
        rollback: RollbackMethod::CloseOwnedDescriptor,
        stabilization: StabilizationMethod::CloseAllOwnedDescriptorsAndQuarantine,
        verification: VerificationMethod::DescriptorOwnershipAndEffectiveConstraint,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::DevicePmQos,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::DeviceHwidFirmware,
        supported_abi: SupportedAbi::DeviceResumeLatencyUs,
        credible_worst_case: "device becomes unresponsive or misses its responsiveness contract",
        semantic_envelope: SemanticEnvelope::Numeric {
            min: 0,
            max: i32::MAX as i64,
            allow_release: true,
        },
        original_capture: OriginalCapture::StartupValue,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedRequestOrRemoveOwnedRequest,
        stabilization: StabilizationMethod::RelaxOwnedRequestOrForceDeviceActive,
        verification: VerificationMethod::ReadbackAndDeviceIdentity,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::CpuEpp,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::CpuPolicy,
        supported_abi: SupportedAbi::CpuFreqEpp,
        credible_worst_case: "sustained performance loss, heat, noise, or battery drain",
        semantic_envelope: SemanticEnvelope::Tokens {
            allowed: EPP_VALUES,
        },
        original_capture: OriginalCapture::StartupValue,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedStartupValue,
        stabilization: StabilizationMethod::AdvertisedBalancedPreference,
        verification: VerificationMethod::ReadEveryCpuPolicy,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::PlatformProfile,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::PlatformFirmware,
        supported_abi: SupportedAbi::AcpiPlatformProfile,
        credible_worst_case: "firmware profile causes severe throttling, heat, or fan noise",
        semantic_envelope: SemanticEnvelope::Tokens {
            allowed: PLATFORM_PROFILES,
        },
        original_capture: OriginalCapture::StartupValue,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedStartupValue,
        stabilization: StabilizationMethod::AdvertisedBalancedProfile,
        verification: VerificationMethod::ReadbackAndAdvertisedChoice,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::RuntimePm,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::DeviceHwidFirmware,
        supported_abi: SupportedAbi::RuntimePmControlDelay,
        credible_worst_case:
            "device disappears, hangs, or loses input, network, audio, or graphics",
        semantic_envelope: SemanticEnvelope::RuntimePm {
            min_delay_ms: 0,
            max_delay_ms: 3_600_000,
        },
        original_capture: OriginalCapture::ControlAndDelay,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedControlAndDelay,
        stabilization: StabilizationMethod::ForceDeviceActive,
        verification: VerificationMethod::ReadbackAndDeviceIdentity,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::SataAlpm,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::ScsiHostController,
        supported_abi: SupportedAbi::SataLinkPowerPolicy,
        credible_worst_case: "storage latency spikes, timeouts, or controller instability",
        semantic_envelope: SemanticEnvelope::Tokens {
            allowed: SATA_POLICIES,
        },
        original_capture: OriginalCapture::StartupValue,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedHostPolicy,
        stabilization: StabilizationMethod::MaxPerformanceIfSupported,
        verification: VerificationMethod::ReadbackAndScsiHostIdentity,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::PcieAspm,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::PciFunctionTopology,
        supported_abi: SupportedAbi::PcieL1Aspm,
        credible_worst_case: "PCIe endpoint instability, timeout, or device loss",
        semantic_envelope: SemanticEnvelope::Boolean,
        original_capture: OriginalCapture::StartupValue,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedLinkState,
        stabilization: StabilizationMethod::DisableOptidEnabledDeeperState,
        verification: VerificationMethod::ReadbackAndPciTopology,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::Backlight,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::DisplayPanel,
        supported_abi: SupportedAbi::BacklightBrightness,
        credible_worst_case: "display becomes visually unusable or unsafe for the selected panel",
        semantic_envelope: SemanticEnvelope::BacklightPercent {
            min: 10,
            max: 100,
            requires_hardware_floor: true,
        },
        original_capture: OriginalCapture::UserOwnedRawValue,
        ownership_rule: OwnershipRule::UserOwnedCompareLastConfirmed,
        rollback: RollbackMethod::RestoreCapturedUserBrightness,
        stabilization: StabilizationMethod::HardwareVerifiedVisibilityFloor,
        verification: VerificationMethod::ReadbackAndPanelIdentity,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::VmSysctl,
        implementation: ImplementationState::Current,
        stable_identity: StableIdentity::BootPolicy,
        supported_abi: SupportedAbi::ProcVmSysctl,
        credible_worst_case: "OOM pressure, writeback stalls, or severe responsiveness loss",
        semantic_envelope: SemanticEnvelope::VmSysctls { rules: VM_SYSCTLS },
        original_capture: OriginalCapture::BootSnapshot,
        ownership_rule: OwnershipRule::TransactionMemberCompareAll,
        rollback: RollbackMethod::RestoreCapturedBootValues,
        stabilization: StabilizationMethod::RecordedDistributionBootPolicy,
        verification: VerificationMethod::ReadbackEveryTransactionMember,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::PowercapPl1,
        implementation: ImplementationState::Planned,
        stable_identity: StableIdentity::CpuPackageFirmware,
        supported_abi: SupportedAbi::PowercapConstraint,
        credible_worst_case: "severe throttling, heat, charger stress, or unstable package power",
        semantic_envelope: SemanticEnvelope::PowercapPercent {
            min: 1,
            max: 100,
            requires_reviewed_cap: true,
        },
        original_capture: OriginalCapture::FirmwareStartupLimit,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedFirmwareLimit,
        stabilization: StabilizationMethod::StopWritingOrReviewedConservativeCap,
        verification: VerificationMethod::BoundsReadbackPackageIdentityTelemetry,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
    LeverContract {
        lever: Lever::DgpuRuntimePm,
        implementation: ImplementationState::Planned,
        stable_identity: StableIdentity::GpuDriverFirmware,
        supported_abi: SupportedAbi::RuntimePmControlDelay,
        credible_worst_case: "GPU loss, display disruption, driver hang, or failed resume",
        semantic_envelope: SemanticEnvelope::RuntimePm {
            min_delay_ms: 0,
            max_delay_ms: 3_600_000,
        },
        original_capture: OriginalCapture::ControlAndDelay,
        ownership_rule: OwnershipRule::CompareLastConfirmedRelinquishOnDrift,
        rollback: RollbackMethod::RestoreCapturedControlAndDelay,
        stabilization: StabilizationMethod::ForceDeviceActive,
        verification: VerificationMethod::ReadbackGpuDriverFirmwareIdentity,
        unsupported: UnsupportedBehavior::DenyActuationAllowObservation,
    },
];

pub fn contracts() -> &'static [LeverContract] {
    &CONTRACTS
}

pub fn contract_for(lever: Lever) -> &'static LeverContract {
    CONTRACTS
        .iter()
        .find(|contract| contract.lever == lever)
        .expect("all Lever variants have an S1D contract")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    WrongContractCount,
    DuplicateLever,
    MissingLever,
    EmptyWorstCase,
    MalformedEnvelope,
    UnsupportedBehaviorNotFailClosed,
    MalformedRollback,
    MalformedStabilization,
}

pub fn validate_registry() -> Result<(), RegistryError> {
    if CONTRACTS.len() != Lever::ALL.len() {
        return Err(RegistryError::WrongContractCount);
    }
    let ids = CONTRACTS
        .iter()
        .map(|contract| contract.lever)
        .collect::<BTreeSet<_>>();
    if ids.len() != CONTRACTS.len() {
        return Err(RegistryError::DuplicateLever);
    }
    if Lever::ALL.iter().any(|lever| !ids.contains(lever)) {
        return Err(RegistryError::MissingLever);
    }

    for contract in CONTRACTS {
        if contract.credible_worst_case.trim().is_empty() {
            return Err(RegistryError::EmptyWorstCase);
        }
        let Some(representative) = contract.semantic_envelope.representative_value() else {
            return Err(RegistryError::MalformedEnvelope);
        };
        if !contract.semantic_envelope.permits(representative)
            || !contract
                .semantic_envelope
                .is_tightening_of(contract.semantic_envelope)
        {
            return Err(RegistryError::MalformedEnvelope);
        }
        if contract.unsupported != UnsupportedBehavior::DenyActuationAllowObservation {
            return Err(RegistryError::UnsupportedBehaviorNotFailClosed);
        }
        let rollback = contract
            .rollback_plan(true, true)
            .map_err(|_| RegistryError::MalformedRollback)?;
        if rollback.lever != contract.lever
            || rollback.classification != HandbackClassification::Restored
            || rollback.rollback != Some(contract.rollback)
            || rollback.stabilization.is_some()
        {
            return Err(RegistryError::MalformedRollback);
        }
        let stabilization = contract
            .stabilization_plan(true)
            .map_err(|_| RegistryError::MalformedStabilization)?;
        if stabilization.lever != contract.lever
            || stabilization.classification != HandbackClassification::Stabilized
            || stabilization.rollback.is_some()
            || stabilization.stabilization != Some(contract.stabilization)
        {
            return Err(RegistryError::MalformedStabilization);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(value: EnvelopeValue<'_>) -> ActuationEvidence<'_> {
        ActuationEvidence {
            stable_identity_verified: true,
            original_value_captured: true,
            verification_available: true,
            value: Some(value),
        }
    }

    #[test]
    fn s1d_registry_contains_all_eleven_complete_lever_contracts() {
        assert_eq!(contracts().len(), 11);
        assert_eq!(validate_registry(), Ok(()));
        assert_eq!(
            contracts()
                .iter()
                .map(|contract| contract.lever)
                .collect::<BTreeSet<_>>(),
            Lever::ALL.into_iter().collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn s1d_fixture_requires_every_field_for_every_lever() {
        let fixture: toml::Value = toml::from_str(include_str!(
            "../tests/fixtures/s1d-lever-contracts-v1.toml"
        ))
        .expect("S1D fixture must parse");
        let rows = fixture
            .get("lever")
            .and_then(toml::Value::as_array)
            .expect("fixture has [[lever]] rows");
        assert_eq!(rows.len(), 11);
        let required = [
            "id",
            "implementation",
            "stable_identity",
            "supported_abi",
            "credible_worst_case",
            "semantic_envelope",
            "original_capture",
            "ownership_drift_rule",
            "rollback",
            "stabilization",
            "verification",
            "unsupported_behavior",
        ];
        let mut fixture_ids = BTreeSet::new();
        for row in rows {
            let table = row.as_table().expect("lever row is a table");
            for field in required {
                let value = table
                    .get(field)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_else(|| panic!("missing S1D fixture field {field}"));
                assert!(!value.trim().is_empty(), "empty S1D fixture field {field}");
            }
            fixture_ids.insert(table["id"].as_str().expect("id is a string"));
        }
        assert_eq!(
            fixture_ids,
            Lever::ALL
                .iter()
                .map(|lever| lever.as_str())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn s1d_each_lever_has_exact_rollback_and_distinct_stabilization() {
        for contract in CONTRACTS {
            assert_eq!(
                contract.rollback_plan(false, true),
                Err(HandbackError::OriginalMissing)
            );
            assert_eq!(
                contract.rollback_plan(true, false),
                Err(HandbackError::OriginalUnverified)
            );
            let rollback = contract
                .rollback_plan(true, true)
                .expect("verified original permits rollback");
            assert_eq!(rollback.lever, contract.lever);
            assert_eq!(rollback.classification, HandbackClassification::Restored);
            assert_eq!(rollback.rollback, Some(contract.rollback));
            assert!(rollback.stabilization.is_none());

            let stabilization = contract
                .stabilization_plan(true)
                .expect("supporting evidence permits stabilization");
            assert_eq!(stabilization.lever, contract.lever);
            assert_eq!(
                stabilization.classification,
                HandbackClassification::Stabilized
            );
            assert!(stabilization.rollback.is_none());
            assert_eq!(stabilization.stabilization, Some(contract.stabilization));
        }
    }

    #[test]
    fn s1d_missing_identity_original_envelope_or_verification_denies_actuation() {
        let lever = Lever::VmSysctl;
        let value = EnvelopeValue::VmSysctl {
            name: "swappiness",
            value: 100,
        };
        let cases = [
            (
                ActuationEvidence {
                    stable_identity_verified: false,
                    ..complete(value)
                },
                AuthorizationDenial::StableIdentityMissing,
            ),
            (
                ActuationEvidence {
                    original_value_captured: false,
                    ..complete(value)
                },
                AuthorizationDenial::OriginalValueMissing,
            ),
            (
                ActuationEvidence {
                    value: None,
                    ..complete(value)
                },
                AuthorizationDenial::EnvelopeValueMissing,
            ),
            (
                ActuationEvidence {
                    verification_available: false,
                    ..complete(value)
                },
                AuthorizationDenial::VerificationUnavailable,
            ),
        ];
        for (evidence, expected) in cases {
            assert_eq!(
                authorize(lever, evidence),
                AuthorizationDecision::Denied(expected)
            );
        }
        assert_eq!(
            authorize(lever, complete(value)),
            AuthorizationDecision::Allowed
        );
    }

    #[test]
    fn s1d_missing_stabilization_evidence_fails_closed() {
        for contract in CONTRACTS {
            if contract.stabilization.requires_external_evidence() {
                assert_eq!(
                    contract.stabilization_plan(false),
                    Err(HandbackError::StabilizationEvidenceMissing)
                );
            }
        }
    }

    #[test]
    fn s1d_tightening_an_envelope_never_authorizes_a_denied_value() {
        let broader = SemanticEnvelope::BacklightPercent {
            min: 10,
            max: 100,
            requires_hardware_floor: true,
        };
        let tighter = SemanticEnvelope::BacklightPercent {
            min: 40,
            max: 80,
            requires_hardware_floor: true,
        };
        assert!(tighter.is_tightening_of(broader));
        for value in 0_u8..=100 {
            if !broader.permits(EnvelopeValue::BacklightPercent(value)) {
                assert!(!tighter.permits(EnvelopeValue::BacklightPercent(value)));
            }
        }
        assert!(broader.permits(EnvelopeValue::BacklightPercent(20)));
        assert!(!tighter.permits(EnvelopeValue::BacklightPercent(20)));
    }

    #[test]
    fn s1d_current_defaults_are_allowed_and_unknown_values_fail_closed() {
        let accepted = [
            (
                Lever::CpuEpp,
                complete(EnvelopeValue::Token("balance_power")),
            ),
            (
                Lever::PlatformProfile,
                complete(EnvelopeValue::Token("balanced")),
            ),
            (
                Lever::RuntimePm,
                complete(EnvelopeValue::RuntimePm {
                    control: "auto",
                    delay_ms: 2000,
                }),
            ),
            (
                Lever::SataAlpm,
                complete(EnvelopeValue::Token("med_power_with_dipm")),
            ),
            (
                Lever::Backlight,
                complete(EnvelopeValue::BacklightPercent(40)),
            ),
            (
                Lever::VmSysctl,
                complete(EnvelopeValue::VmSysctl {
                    name: "swappiness",
                    value: 100,
                }),
            ),
        ];
        for (lever, evidence) in accepted {
            assert_eq!(authorize(lever, evidence), AuthorizationDecision::Allowed);
        }

        let denied = [
            (
                Lever::CpuEpp,
                complete(EnvelopeValue::Token("turbo_forever")),
            ),
            (
                Lever::RuntimePm,
                complete(EnvelopeValue::RuntimePm {
                    control: "auto",
                    delay_ms: -1,
                }),
            ),
            (
                Lever::Backlight,
                complete(EnvelopeValue::BacklightPercent(0)),
            ),
            (
                Lever::VmSysctl,
                complete(EnvelopeValue::VmSysctl {
                    name: "unknown_knob",
                    value: 1,
                }),
            ),
        ];
        for (lever, evidence) in denied {
            assert_eq!(
                authorize(lever, evidence),
                AuthorizationDecision::Denied(AuthorizationDenial::ValueOutsideEnvelope)
            );
        }
    }

    #[test]
    fn s1d_planned_levers_remain_fail_closed_until_their_write_packages_exist() {
        for (lever, value) in [
            (Lever::PowercapPl1, EnvelopeValue::PowercapPercent(50)),
            (
                Lever::DgpuRuntimePm,
                EnvelopeValue::RuntimePm {
                    control: "auto",
                    delay_ms: 2000,
                },
            ),
        ] {
            assert_eq!(
                contract_for(lever).implementation,
                ImplementationState::Planned
            );
            assert_eq!(
                authorize(lever, complete(value)),
                AuthorizationDecision::Denied(AuthorizationDenial::LeverNotImplemented)
            );
        }
    }
}
