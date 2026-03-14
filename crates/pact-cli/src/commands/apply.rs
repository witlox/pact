//! Apply command — parse a declarative config spec and submit to journal.
//!
//! Spec format (TOML):
//! ```toml
//! [vcluster.ml-training.sysctl]
//! "vm.nr_hugepages" = "1024"
//! "vm.swappiness" = "10"
//!
//! [vcluster.ml-training.mounts]
//! "/local-scratch" = { type = "nfs", source = "storage03:/scratch" }
//!
//! [vcluster.ml-training.services.nvidia-persistenced]
//! state = "running"
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use pact_common::types::{DeltaAction, DeltaItem, StateDelta};

/// Top-level spec: `[vcluster.<name>.<section>]`
#[derive(Debug, Deserialize)]
pub struct ApplySpec {
    #[serde(default)]
    pub vcluster: BTreeMap<String, VClusterSpec>,
}

/// Per-vCluster spec sections.
#[derive(Debug, Default, Deserialize)]
pub struct VClusterSpec {
    #[serde(default)]
    pub sysctl: BTreeMap<String, String>,
    #[serde(default)]
    pub mounts: BTreeMap<String, MountSpec>,
    #[serde(default)]
    pub files: BTreeMap<String, FileSpec>,
    #[serde(default)]
    pub services: BTreeMap<String, ServiceSpec>,
    #[serde(default)]
    pub network: BTreeMap<String, String>,
    #[serde(default)]
    pub packages: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct MountSpec {
    #[serde(rename = "type")]
    pub fs_type: String,
    pub source: String,
    #[serde(default)]
    pub options: String,
}

#[derive(Debug, Deserialize)]
pub struct FileSpec {
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceSpec {
    pub state: String,
}

/// Load and parse an apply spec from a TOML file.
pub fn load_spec(path: &Path) -> anyhow::Result<ApplySpec> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read spec file {}: {e}", path.display()))?;
    let spec: ApplySpec = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("invalid spec format in {}: {e}", path.display()))?;
    Ok(spec)
}

/// Convert a vCluster spec into a StateDelta for journal submission.
pub fn spec_to_delta(spec: &VClusterSpec) -> StateDelta {
    let mut delta = StateDelta::default();

    for (key, value) in &spec.sysctl {
        delta.kernel.push(DeltaItem {
            action: DeltaAction::Modify,
            key: key.clone(),
            value: Some(value.clone()),
            previous: None,
        });
    }

    for (path, mount) in &spec.mounts {
        delta.mounts.push(DeltaItem {
            action: DeltaAction::Add,
            key: path.clone(),
            value: Some(format!("{}:{} ({})", mount.fs_type, mount.source, mount.options)),
            previous: None,
        });
    }

    for (path, file) in &spec.files {
        delta.files.push(DeltaItem {
            action: DeltaAction::Modify,
            key: path.clone(),
            value: file.content_hash.clone(),
            previous: file.owner.clone(),
        });
    }

    for (name, svc) in &spec.services {
        delta.services.push(DeltaItem {
            action: DeltaAction::Modify,
            key: name.clone(),
            value: Some(svc.state.clone()),
            previous: None,
        });
    }

    for (iface, detail) in &spec.network {
        delta.network.push(DeltaItem {
            action: DeltaAction::Modify,
            key: iface.clone(),
            value: Some(detail.clone()),
            previous: None,
        });
    }

    for (name, version) in &spec.packages {
        delta.packages.push(DeltaItem {
            action: DeltaAction::Add,
            key: name.clone(),
            value: Some(version.clone()),
            previous: None,
        });
    }

    delta
}

/// Format a summary of changes from a spec.
pub fn format_spec_summary(spec: &ApplySpec) -> String {
    let mut lines = Vec::new();
    for (vc_name, vc_spec) in &spec.vcluster {
        let mut changes = 0;
        changes += vc_spec.sysctl.len();
        changes += vc_spec.mounts.len();
        changes += vc_spec.files.len();
        changes += vc_spec.services.len();
        changes += vc_spec.network.len();
        changes += vc_spec.packages.len();
        lines.push(format!("vCluster {vc_name}: {changes} changes"));

        for (key, val) in &vc_spec.sysctl {
            lines.push(format!("  sysctl: {key} = {val}"));
        }
        for (path, mount) in &vc_spec.mounts {
            lines.push(format!("  mount: {path} ({}:{})", mount.fs_type, mount.source));
        }
        for (name, svc) in &vc_spec.services {
            lines.push(format!("  service: {name} → {}", svc.state));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sysctl_spec() {
        let toml = r#"
        [vcluster.ml-training.sysctl]
        "vm.nr_hugepages" = "1024"
        "vm.swappiness" = "10"
        "#;
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        let vc = &spec.vcluster["ml-training"];
        assert_eq!(vc.sysctl.len(), 2);
        assert_eq!(vc.sysctl["vm.nr_hugepages"], "1024");
    }

    #[test]
    fn parse_mount_spec() {
        let toml = r#"
        [vcluster.ml-training.mounts."/local-scratch"]
        type = "nfs"
        source = "storage03:/scratch"
        options = "rw,noatime"
        "#;
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        let mount = &spec.vcluster["ml-training"].mounts["/local-scratch"];
        assert_eq!(mount.fs_type, "nfs");
        assert_eq!(mount.source, "storage03:/scratch");
    }

    #[test]
    fn parse_service_spec() {
        let toml = r#"
        [vcluster.ml-training.services.nvidia-persistenced]
        state = "running"

        [vcluster.ml-training.services.chronyd]
        state = "running"
        "#;
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        assert_eq!(spec.vcluster["ml-training"].services.len(), 2);
    }

    #[test]
    fn spec_to_delta_converts_all_categories() {
        let toml = r#"
        [vcluster.ml-training.sysctl]
        "vm.nr_hugepages" = "1024"

        [vcluster.ml-training.mounts."/scratch"]
        type = "nfs"
        source = "storage:/scratch"

        [vcluster.ml-training.services.chronyd]
        state = "running"

        [vcluster.ml-training.packages]
        cuda = "12.4"
        "#;
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        let delta = spec_to_delta(&spec.vcluster["ml-training"]);
        assert_eq!(delta.kernel.len(), 1);
        assert_eq!(delta.mounts.len(), 1);
        assert_eq!(delta.services.len(), 1);
        assert_eq!(delta.packages.len(), 1);
    }

    #[test]
    fn format_summary_shows_changes() {
        let toml = r#"
        [vcluster.ml-training.sysctl]
        "vm.nr_hugepages" = "1024"

        [vcluster.ml-training.services.chronyd]
        state = "running"
        "#;
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        let summary = format_spec_summary(&spec);
        assert!(summary.contains("ml-training: 2 changes"));
        assert!(summary.contains("sysctl: vm.nr_hugepages"));
        assert!(summary.contains("service: chronyd"));
    }

    #[test]
    fn empty_spec_parses() {
        let toml = "";
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        assert!(spec.vcluster.is_empty());
    }

    #[test]
    fn multi_vcluster_spec() {
        let toml = r#"
        [vcluster.ml-training.sysctl]
        "vm.nr_hugepages" = "1024"

        [vcluster.dev-sandbox.sysctl]
        "vm.swappiness" = "60"
        "#;
        let spec: ApplySpec = toml::from_str(toml).unwrap();
        assert_eq!(spec.vcluster.len(), 2);
    }
}
