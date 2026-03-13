//! Shared helper functions for step implementations.

use pact_common::types::{AdminOperationType, ConfigState, DriftVector, EntryType};

pub fn parse_config_state(s: &str) -> ConfigState {
    match s {
        "ObserveOnly" => ConfigState::ObserveOnly,
        "Committed" => ConfigState::Committed,
        "Drifted" => ConfigState::Drifted,
        "Converging" => ConfigState::Converging,
        "Emergency" => ConfigState::Emergency,
        _ => panic!("unknown config state: {s}"),
    }
}

pub fn parse_entry_type(s: &str) -> EntryType {
    match s {
        "Commit" => EntryType::Commit,
        "Rollback" => EntryType::Rollback,
        "AutoConverge" => EntryType::AutoConverge,
        "DriftDetected" => EntryType::DriftDetected,
        "CapabilityChange" => EntryType::CapabilityChange,
        "PolicyUpdate" => EntryType::PolicyUpdate,
        "BootConfig" => EntryType::BootConfig,
        "EmergencyStart" => EntryType::EmergencyStart,
        "EmergencyEnd" => EntryType::EmergencyEnd,
        "ExecLog" => EntryType::ExecLog,
        "ShellSession" => EntryType::ShellSession,
        "ServiceLifecycle" => EntryType::ServiceLifecycle,
        _ => panic!("unknown entry type: {s}"),
    }
}

pub fn parse_admin_op_type(s: &str) -> AdminOperationType {
    match s {
        "Exec" => AdminOperationType::Exec,
        "ShellSessionStart" => AdminOperationType::ShellSessionStart,
        "ShellSessionEnd" => AdminOperationType::ShellSessionEnd,
        "ServiceStart" => AdminOperationType::ServiceStart,
        "ServiceStop" => AdminOperationType::ServiceStop,
        "ServiceRestart" => AdminOperationType::ServiceRestart,
        "EmergencyStart" => AdminOperationType::EmergencyStart,
        "EmergencyEnd" => AdminOperationType::EmergencyEnd,
        _ => panic!("unknown admin op type: {s}"),
    }
}

pub fn set_drift_dimension(v: &mut DriftVector, dim: &str, val: f64) {
    match dim {
        "mounts" => v.mounts = val,
        "files" => v.files = val,
        "network" => v.network = val,
        "services" => v.services = val,
        "kernel" => v.kernel = val,
        "packages" => v.packages = val,
        "gpu" => v.gpu = val,
        _ => panic!("unknown drift dimension: {dim}"),
    }
}

pub fn get_drift_dimension(v: &DriftVector, dim: &str) -> f64 {
    match dim {
        "mounts" => v.mounts,
        "files" => v.files,
        "network" => v.network,
        "services" => v.services,
        "kernel" => v.kernel,
        "packages" => v.packages,
        "gpu" => v.gpu,
        _ => panic!("unknown drift dimension: {dim}"),
    }
}

pub fn md5_simple(s: &str) -> u64 {
    let mut hash: u64 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    hash
}

pub fn default_whitelist() -> Vec<String> {
    vec![
        "nvidia-smi",
        "dmesg",
        "lspci",
        "ip",
        "ss",
        "cat",
        "journalctl",
        "mount",
        "df",
        "free",
        "top",
        "ps",
        "lsmod",
        "sysctl",
        "uname",
        "hostname",
        "date",
        "uptime",
        "who",
        "w",
        "last",
        "netstat",
        "ethtool",
        "lsblk",
        "blkid",
        "findmnt",
        "nproc",
        "lscpu",
        "numactl",
        "dmidecode",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Map feature-file action names to real RBAC action constants.
pub fn map_action(action: &str) -> &str {
    match action {
        "emergency" => "emergency_start",
        other => other,
    }
}
