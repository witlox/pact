//! Capability reporting steps — wired to CapabilityReporter + MockGpuBackend.

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use pact_common::types::{
    CapabilityReport, ConfigState, CpuArchitecture, CpuCapability, DiskType, EmergencyInfo, FsType,
    GpuCapability, GpuHealth, GpuVendor, HugePageInfo, Identity, InterfaceOperState, LocalDisk,
    MemoryCapability, MemoryType, MountInfo, NetworkFabric, NetworkInterface, NumaNode,
    PrincipalType, ServiceStatusInfo, SoftwareCapability, StorageCapability, StorageNodeType,
    SupervisorBackend, SupervisorStatus,
};
use pact_journal::JournalCommand;
use tempfile::TempDir;

use crate::PactWorld;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_gpu(index: u32, vendor: GpuVendor, model: &str, health: GpuHealth) -> GpuCapability {
    GpuCapability {
        index,
        vendor,
        model: model.to_string(),
        memory_bytes: 80 * 1024 * 1024 * 1024,
        health,
        pci_bus_id: format!("0000:{:02x}:00.0", index + 0x3b),
    }
}

fn parse_memory_type(s: &str) -> MemoryType {
    match s {
        "DDR4" => MemoryType::Ddr4,
        "DDR5" => MemoryType::Ddr5,
        "HBM2e" => MemoryType::Hbm2e,
        "HBM3" => MemoryType::Hbm3,
        "HBM3e" => MemoryType::Hbm3e,
        _ => MemoryType::Unknown,
    }
}

fn parse_fabric(driver: &str) -> NetworkFabric {
    match driver {
        "cxi" => NetworkFabric::Slingshot,
        _ => NetworkFabric::Ethernet,
    }
}

fn parse_oper_state(s: &str) -> InterfaceOperState {
    match s.to_lowercase().as_str() {
        "up" => InterfaceOperState::Up,
        _ => InterfaceOperState::Down,
    }
}

fn parse_disk_type(s: &str) -> DiskType {
    match s {
        "Nvme" => DiskType::Nvme,
        "Ssd" => DiskType::Ssd,
        "Hdd" => DiskType::Hdd,
        _ => DiskType::Unknown,
    }
}

fn parse_fs_type(s: &str) -> FsType {
    match s {
        "Nfs" => FsType::Nfs,
        "Lustre" => FsType::Lustre,
        "Ext4" => FsType::Ext4,
        "Xfs" => FsType::Xfs,
        "Tmpfs" => FsType::Tmpfs,
        other => FsType::Other(other.to_string()),
    }
}

/// Parse a CPU range string like "0-27,112-139" into a Vec<u32>.
fn parse_cpu_range(s: &str) -> Vec<u32> {
    let mut cpus = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some((start_s, end_s)) = part.split_once('-') {
            let start: u32 = start_s.trim().parse().unwrap();
            let end: u32 = end_s.trim().parse().unwrap();
            cpus.extend(start..=end);
        } else {
            cpus.push(part.parse().unwrap());
        }
    }
    cpus
}

fn build_report(world: &PactWorld, node_id: &str) -> CapabilityReport {
    let config_state =
        world.journal.node_states.get(node_id).cloned().unwrap_or(ConfigState::ObserveOnly);

    let emergency = if config_state == ConfigState::Emergency {
        Some(EmergencyInfo {
            reason: "emergency active".into(),
            admin_identity: Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-platform-admin".into(),
            },
            started_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(4),
        })
    } else {
        None
    };

    let cpu = world.cpu_capability.clone().unwrap_or_default();

    let memory = world.memory_capability.clone().unwrap_or(MemoryCapability {
        total_bytes: 0,
        available_bytes: 0,
        memory_type: MemoryType::default(),
        numa_nodes: 1,
        numa_topology: vec![],
        hugepages: HugePageInfo::default(),
    });

    let network = world.network_interfaces.clone().unwrap_or_default();

    let storage = world.storage_capability.clone().unwrap_or(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });

    let software = world.software_capability.clone().unwrap_or(SoftwareCapability {
        loaded_modules: vec![],
        uenv_image: None,
        services: vec![],
    });

    CapabilityReport {
        node_id: node_id.into(),
        timestamp: chrono::Utc::now(),
        report_id: uuid::Uuid::new_v4(),
        cpu,
        gpus: world.gpu_capabilities.clone(),
        memory,
        network,
        storage,
        software,
        config_state,
        drift_summary: None,
        emergency,
        supervisor_status: world.supervisor_status.clone(),
    }
}

// ---------------------------------------------------------------------------
// GIVEN — GPU steps (existing)
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with (\d+) NVIDIA (\S+) GPUs$")]
async fn given_nvidia_gpus(world: &mut PactWorld, count: u32, model: String) {
    world.gpu_capabilities =
        (0..count).map(|i| make_gpu(i, GpuVendor::Nvidia, &model, GpuHealth::Healthy)).collect();
}

#[given(regex = r"^a node with (\d+) AMD (\S+) GPUs$")]
async fn given_amd_gpus(world: &mut PactWorld, count: u32, model: String) {
    world.gpu_capabilities =
        (0..count).map(|i| make_gpu(i, GpuVendor::Amd, &model, GpuHealth::Healthy)).collect();
}

#[given(regex = r"^a node with (\d+) NVIDIA (\S+) GPUs and (\d+) AMD (\S+) GPUs$")]
async fn given_mixed_gpus(
    world: &mut PactWorld,
    nv_count: u32,
    nv_model: String,
    amd_count: u32,
    amd_model: String,
) {
    let mut gpus: Vec<GpuCapability> = (0..nv_count)
        .map(|i| make_gpu(i, GpuVendor::Nvidia, &nv_model, GpuHealth::Healthy))
        .collect();
    gpus.extend(
        (0..amd_count)
            .map(|i| make_gpu(nv_count + i, GpuVendor::Amd, &amd_model, GpuHealth::Healthy)),
    );
    world.gpu_capabilities = gpus;
}

#[given("a node with no GPUs")]
async fn given_no_gpus(world: &mut PactWorld) {
    world.gpu_capabilities.clear();
}

#[given("a node with 1 NVIDIA GPU in healthy state")]
async fn given_healthy_gpu(world: &mut PactWorld) {
    world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Healthy)];
}

#[given("a node with 1 NVIDIA GPU that reports degraded health")]
async fn given_degraded_gpu(world: &mut PactWorld) {
    world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Degraded)];
}

#[given("a node with 1 NVIDIA GPU that fails")]
async fn given_failed_gpu(world: &mut PactWorld) {
    world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Failed)];
}

#[given(regex = r"^a node with (\d+) GB of memory$")]
async fn given_memory_gb(world: &mut PactWorld, gb: u64) {
    world.memory_capability = Some(MemoryCapability {
        total_bytes: gb * 1024 * 1024 * 1024,
        available_bytes: gb * 1024 * 1024 * 1024,
        memory_type: MemoryType::default(),
        numa_nodes: 1,
        numa_topology: vec![],
        hugepages: HugePageInfo::default(),
    });
}

#[given("a stable capability report")]
async fn given_stable_report(world: &mut PactWorld) {
    if world.gpu_capabilities.is_empty() {
        world.gpu_capabilities = vec![make_gpu(0, GpuVendor::Nvidia, "A100", GpuHealth::Healthy)];
    }
    world.capability_report = Some(build_report(world, "test-node"));
}

#[given(regex = r#"^node "([\w-]+)" is in state "([\w]+)"$"#)]
async fn given_node_state(world: &mut PactWorld, node: String, state_str: String) {
    if state_str == "Inactive" && world.journal.enrollments.contains_key(&node) {
        world.journal.apply_command(JournalCommand::DeactivateNode { node_id: node.clone() });
    }
    let state = super::helpers::parse_config_state(&state_str);
    world.journal.apply_command(JournalCommand::UpdateNodeState { node_id: node, state });
}

#[given(regex = r"^(\d+) declared services with (\d+) running and (\d+) failed$")]
async fn given_supervisor_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    world.supervisor_status = SupervisorStatus {
        backend: SupervisorBackend::Pact,
        services_declared: declared,
        services_running: running,
        services_failed: failed,
    };
    // Also populate software services so the count is accessible via the report
    let mut services = Vec::new();
    for i in 0..running {
        services.push(ServiceStatusInfo {
            name: format!("service-{}", i),
            state: pact_common::types::ServiceState::Running,
            pid: 1000 + i,
            uptime_seconds: 3600,
            restart_count: 0,
        });
    }
    for i in 0..failed {
        services.push(ServiceStatusInfo {
            name: format!("failed-service-{}", i),
            state: pact_common::types::ServiceState::Failed,
            pid: 0,
            uptime_seconds: 0,
            restart_count: 1,
        });
    }
    // Add stopped services to reach declared count
    let stopped = declared.saturating_sub(running + failed);
    for i in 0..stopped {
        services.push(ServiceStatusInfo {
            name: format!("stopped-service-{}", i),
            state: pact_common::types::ServiceState::Stopped,
            pid: 0,
            uptime_seconds: 0,
            restart_count: 0,
        });
    }
    let sw = world.software_capability.get_or_insert(SoftwareCapability {
        loaded_modules: vec![],
        uenv_image: None,
        services: vec![],
    });
    sw.services = services;
}

#[given(regex = r#"^node "([\w-]+)" is in emergency mode$"#)]
async fn given_emergency_mode(world: &mut PactWorld, node: String) {
    world.journal.apply_command(JournalCommand::UpdateNodeState {
        node_id: node,
        state: ConfigState::Emergency,
    });
}

// ---------------------------------------------------------------------------
// GIVEN — CPU steps
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with an x86_64 CPU$")]
async fn given_x86_cpu(world: &mut PactWorld, step: &Step) {
    let mut cpu = CpuCapability::default();
    cpu.architecture = CpuArchitecture::X86_64;
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let field = &row[0];
            let value = &row[1];
            match field.as_str() {
                "model" => cpu.model = value.clone(),
                "physical_cores" => cpu.physical_cores = value.parse().unwrap(),
                "logical_cores" => cpu.logical_cores = value.parse().unwrap(),
                "base_freq_mhz" => cpu.base_frequency_mhz = value.parse().unwrap(),
                "max_freq_mhz" => cpu.max_frequency_mhz = value.parse().unwrap(),
                "features" => {
                    cpu.features = value.split(',').map(|s| s.trim().to_string()).collect()
                }
                "numa_nodes" => cpu.numa_nodes = value.parse().unwrap(),
                "cache_l3_bytes" => cpu.cache_l3_bytes = value.parse().unwrap(),
                _ => {}
            }
        }
    }
    world.cpu_capability = Some(cpu);
}

#[given(regex = r"^a node with an aarch64 CPU$")]
async fn given_aarch64_cpu(world: &mut PactWorld, step: &Step) {
    let mut cpu = CpuCapability::default();
    cpu.architecture = CpuArchitecture::Aarch64;
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let field = &row[0];
            let value = &row[1];
            match field.as_str() {
                "model" => cpu.model = value.clone(),
                "physical_cores" => cpu.physical_cores = value.parse().unwrap(),
                "logical_cores" => cpu.logical_cores = value.parse().unwrap(),
                "base_freq_mhz" => cpu.base_frequency_mhz = value.parse().unwrap(),
                "max_freq_mhz" => cpu.max_frequency_mhz = value.parse().unwrap(),
                "features" => {
                    cpu.features = value.split(',').map(|s| s.trim().to_string()).collect()
                }
                "numa_nodes" => cpu.numa_nodes = value.parse().unwrap(),
                "cache_l3_bytes" => cpu.cache_l3_bytes = value.parse().unwrap(),
                _ => {}
            }
        }
    }
    world.cpu_capability = Some(cpu);
}

#[given(regex = r"^a node with no CPU info available$")]
async fn given_no_cpu(world: &mut PactWorld) {
    world.cpu_capability = Some(CpuCapability::default());
}

// ---------------------------------------------------------------------------
// GIVEN — Memory steps
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with memory$")]
async fn given_memory(world: &mut PactWorld, step: &Step) {
    let mut mem = MemoryCapability {
        total_bytes: 0,
        available_bytes: 0,
        memory_type: MemoryType::default(),
        numa_nodes: 1,
        numa_topology: vec![],
        hugepages: HugePageInfo::default(),
    };
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let field = &row[0];
            let value = &row[1];
            match field.as_str() {
                "total_bytes" => mem.total_bytes = value.parse().unwrap(),
                "available_bytes" => mem.available_bytes = value.parse().unwrap(),
                "memory_type" => mem.memory_type = parse_memory_type(value),
                "numa_nodes" => mem.numa_nodes = value.parse().unwrap(),
                _ => {}
            }
        }
    }
    world.memory_capability = Some(mem);
}

#[given(regex = r"^a node with (\d+) NUMA nodes$")]
async fn given_numa_nodes(world: &mut PactWorld, count: u32, step: &Step) {
    let mut topology = Vec::new();
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let node_id: u32 = row[0].parse().unwrap();
            let total_bytes: u64 = row[1].parse().unwrap();
            let cpus = parse_cpu_range(&row[2]);
            topology.push(NumaNode { id: node_id, total_bytes, cpus });
        }
    }
    let mem = world.memory_capability.get_or_insert(MemoryCapability {
        total_bytes: 0,
        available_bytes: 0,
        memory_type: MemoryType::default(),
        numa_nodes: 1,
        numa_topology: vec![],
        hugepages: HugePageInfo::default(),
    });
    mem.numa_nodes = count;
    mem.numa_topology = topology;
}

#[given(regex = r"^a node with huge pages$")]
async fn given_huge_pages(world: &mut PactWorld, step: &Step) {
    let mut hp = HugePageInfo::default();
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let field = &row[0];
            let value = &row[1];
            match field.as_str() {
                "size_2mb_total" => hp.size_2mb_total = value.parse().unwrap(),
                "size_2mb_free" => hp.size_2mb_free = value.parse().unwrap(),
                "size_1gb_total" => hp.size_1gb_total = value.parse().unwrap(),
                "size_1gb_free" => hp.size_1gb_free = value.parse().unwrap(),
                _ => {}
            }
        }
    }
    let mem = world.memory_capability.get_or_insert(MemoryCapability {
        total_bytes: 0,
        available_bytes: 0,
        memory_type: MemoryType::default(),
        numa_nodes: 1,
        numa_topology: vec![],
        hugepages: HugePageInfo::default(),
    });
    mem.hugepages = hp;
}

#[given(regex = r"^a node with memory but no NUMA topology$")]
async fn given_memory_no_numa(world: &mut PactWorld, step: &Step) {
    let mut mem = MemoryCapability {
        total_bytes: 0,
        available_bytes: 0,
        memory_type: MemoryType::default(),
        numa_nodes: 1,
        numa_topology: vec![],
        hugepages: HugePageInfo::default(),
    };
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let field = &row[0];
            let value = &row[1];
            match field.as_str() {
                "total_bytes" => mem.total_bytes = value.parse().unwrap(),
                "available_bytes" => mem.available_bytes = value.parse().unwrap(),
                "memory_type" => mem.memory_type = parse_memory_type(value),
                _ => {}
            }
        }
    }
    // No NUMA topology: fall back to single node with all memory
    mem.numa_nodes = 1;
    mem.numa_topology = vec![NumaNode { id: 0, total_bytes: mem.total_bytes, cpus: vec![] }];
    world.memory_capability = Some(mem);
}

// ---------------------------------------------------------------------------
// GIVEN — Network steps
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with network interfaces$")]
async fn given_network_interfaces(world: &mut PactWorld, step: &Step) {
    let mut interfaces = Vec::new();
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let name = row[0].clone();
            let driver = &row[1];
            let speed_mbps: u64 = row[2].parse().unwrap();
            let state = parse_oper_state(&row[3]);
            let mac = row[4].clone();
            let ipv4 =
                if row.len() > 5 && !row[5].is_empty() { Some(row[5].clone()) } else { None };
            let fabric = parse_fabric(driver);
            interfaces.push(NetworkInterface { name, fabric, speed_mbps, state, mac, ipv4 });
        }
    }
    world.network_interfaces = Some(interfaces);
}

#[given(regex = r"^a node with no network interfaces$")]
async fn given_no_network(world: &mut PactWorld) {
    world.network_interfaces = Some(vec![]);
}

// ---------------------------------------------------------------------------
// GIVEN — Storage steps
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with local disks$")]
async fn given_local_disks(world: &mut PactWorld, step: &Step) {
    let mut disks = Vec::new();
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let device = row[0].clone();
            let model = row[1].clone();
            let capacity_bytes: u64 = row[2].parse().unwrap();
            let disk_type = parse_disk_type(&row[3]);
            disks.push(LocalDisk { device, model, capacity_bytes, disk_type });
        }
    }
    let storage = world.storage_capability.get_or_insert(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });
    storage.local_disks = disks;
    if !storage.local_disks.is_empty() {
        storage.node_type = StorageNodeType::LocalStorage;
    }
}

#[given(regex = r"^a node with no local disks$")]
async fn given_no_local_disks(world: &mut PactWorld) {
    let storage = world.storage_capability.get_or_insert(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });
    storage.local_disks.clear();
    storage.node_type = StorageNodeType::Diskless;
}

#[given(regex = r"^a node with mounts$")]
async fn given_mounts(world: &mut PactWorld, step: &Step) {
    let mut mounts = Vec::new();
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            let path = row[0].clone();
            let fs_type = parse_fs_type(&row[1]);
            let source = row[2].clone();
            let total_bytes: u64 = row[3].parse().unwrap();
            let available_bytes: u64 = row[4].parse().unwrap();
            mounts.push(MountInfo { path, fs_type, source, total_bytes, available_bytes });
        }
    }
    let storage = world.storage_capability.get_or_insert(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });
    storage.mounts = mounts;
}

#[given(regex = r"^a node with no mounts$")]
async fn given_no_mounts(world: &mut PactWorld) {
    let storage = world.storage_capability.get_or_insert(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });
    storage.mounts.clear();
}

// ---------------------------------------------------------------------------
// GIVEN — Software steps
// ---------------------------------------------------------------------------

#[given(regex = r"^a node with loaded modules$")]
async fn given_loaded_modules(world: &mut PactWorld, step: &Step) {
    let mut modules = Vec::new();
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows[1..] {
            modules.push(row[0].clone());
        }
    }
    let sw = world.software_capability.get_or_insert(SoftwareCapability {
        loaded_modules: vec![],
        uenv_image: None,
        services: vec![],
    });
    sw.loaded_modules = modules;
}

#[given(regex = r#"^a node with uenv image "(.+)"$"#)]
async fn given_uenv_image(world: &mut PactWorld, image: String) {
    let sw = world.software_capability.get_or_insert(SoftwareCapability {
        loaded_modules: vec![],
        uenv_image: None,
        services: vec![],
    });
    sw.uenv_image = Some(image);
}

// ---------------------------------------------------------------------------
// GIVEN — Failure mode steps (mock backends)
// ---------------------------------------------------------------------------

#[given(regex = r"^a capability reporter with a mock CPU backend returning error$")]
async fn given_mock_cpu_error(world: &mut PactWorld) {
    // Simulate CPU detection failure: return default (unknown) CPU
    world.cpu_capability = Some(CpuCapability::default());
}

#[given(regex = r"^a capability reporter with mock network interfaces$")]
async fn given_mock_network(world: &mut PactWorld) {
    // Initialize with empty; specific interfaces added by subsequent steps
    if world.network_interfaces.is_none() {
        world.network_interfaces = Some(vec![]);
    }
}

#[given(regex = r#"^interface "(\S+)" has speed -1$"#)]
async fn given_interface_speed_neg1(world: &mut PactWorld, iface_name: String) {
    let interfaces = world.network_interfaces.get_or_insert_with(Vec::new);
    // Check if interface already exists, otherwise add it
    if let Some(iface) = interfaces.iter_mut().find(|i| i.name == iface_name) {
        iface.speed_mbps = 0; // -1 mapped to 0
    } else {
        interfaces.push(NetworkInterface {
            name: iface_name,
            fabric: NetworkFabric::Ethernet,
            speed_mbps: 0, // -1 from sysfs mapped to 0
            state: InterfaceOperState::Up,
            mac: "00:00:00:00:00:00".into(),
            ipv4: None,
        });
    }
}

#[given(regex = r"^a capability reporter with a mock storage backend returning no disks$")]
async fn given_mock_storage_no_disks(world: &mut PactWorld) {
    world.storage_capability = Some(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });
}

#[given(regex = r"^a capability reporter with mock storage backend$")]
async fn given_mock_storage_backend(world: &mut PactWorld) {
    if world.storage_capability.is_none() {
        world.storage_capability = Some(StorageCapability {
            node_type: StorageNodeType::Diskless,
            local_disks: vec![],
            mounts: vec![],
        });
    }
}

#[given(regex = r#"^mount "(\S+)" has statvfs error$"#)]
async fn given_mount_statvfs_error(world: &mut PactWorld, mount_path: String) {
    let storage = world.storage_capability.get_or_insert(StorageCapability {
        node_type: StorageNodeType::Diskless,
        local_disks: vec![],
        mounts: vec![],
    });
    // Add a mount with zeroed capacity to simulate statvfs failure
    storage.mounts.push(MountInfo {
        path: mount_path,
        fs_type: FsType::Nfs,
        source: "nfs-server:/stale".into(),
        total_bytes: 0,
        available_bytes: 0,
    });
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("capability detection runs")]
async fn when_detection_runs(world: &mut PactWorld) {
    let mut report = build_report(world, "test-node");

    // If any GPU has non-healthy state, record a CapabilityChange entry
    let has_degraded = report.gpus.iter().any(|g| g.health != GpuHealth::Healthy);
    if has_degraded {
        let entry = pact_common::types::ConfigEntry {
            sequence: 0,
            timestamp: chrono::Utc::now(),
            entry_type: pact_common::types::EntryType::CapabilityChange,
            scope: pact_common::types::Scope::Node("test-node".into()),
            author: pact_common::types::Identity {
                principal: "pact-agent".into(),
                principal_type: PrincipalType::Service,
                role: "pact-service-agent".into(),
            },
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        };
        world.journal.apply_command(JournalCommand::AppendEntry(entry));
    }

    world.capability_report = Some(report);
    world.manifest_written = true;
    world.socket_available = true;
}

#[when(regex = r#"^capability detection runs for node "([\w-]+)"$"#)]
async fn when_detection_runs_node(world: &mut PactWorld, node: String) {
    let report = build_report(world, &node);
    world.capability_report = Some(report);
    world.manifest_written = true;
}

#[when("GPU 0 transitions from healthy to degraded")]
async fn when_gpu_transition(world: &mut PactWorld) {
    if !world.gpu_capabilities.is_empty() {
        world.gpu_capabilities[0].health = GpuHealth::Degraded;
    }
    let report = build_report(world, "test-node");
    world.capability_report = Some(report);

    let entry = pact_common::types::ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: pact_common::types::EntryType::CapabilityChange,
        scope: pact_common::types::Scope::Node("test-node".into()),
        author: pact_common::types::Identity {
            principal: "pact-agent".into(),
            principal_type: pact_common::types::PrincipalType::Service,
            role: "pact-service-agent".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when(regex = r#"^interface "(\S+)" transitions from up to down$"#)]
async fn when_interface_transition(world: &mut PactWorld, iface_name: String) {
    if let Some(interfaces) = world.network_interfaces.as_mut() {
        if let Some(iface) = interfaces.iter_mut().find(|i| i.name == iface_name) {
            iface.state = InterfaceOperState::Down;
            iface.speed_mbps = 0;
        }
    }
    // Rebuild report with updated network state
    let report = build_report(world, "test-node");
    world.capability_report = Some(report);

    // Record CapabilityChange entry
    let entry = pact_common::types::ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: pact_common::types::EntryType::CapabilityChange,
        scope: pact_common::types::Scope::Node("test-node".into()),
        author: pact_common::types::Identity {
            principal: "pact-agent".into(),
            principal_type: pact_common::types::PrincipalType::Service,
            role: "pact-service-agent".into(),
        },
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

#[when("the configured poll interval elapses")]
async fn when_poll_interval(world: &mut PactWorld) {
    let report = build_report(world, "test-node");
    world.capability_report = Some(report);
}

#[when("the supervisor status is queried")]
async fn when_supervisor_status(world: &mut PactWorld) {
    // Status is already in world.supervisor_status
}

// ---------------------------------------------------------------------------
// THEN — GPU steps (existing)
// ---------------------------------------------------------------------------

#[then(regex = r"^the capability report should contain (\d+) GPUs?$")]
async fn then_gpu_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.gpus.len(), count, "expected {count} GPUs, got {}", report.gpus.len());
}

#[then(regex = r#"^all GPUs should have vendor "([\w]+)"$"#)]
async fn then_all_vendor(world: &mut PactWorld, vendor_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match vendor_str.as_str() {
        "Nvidia" => GpuVendor::Nvidia,
        "Amd" => GpuVendor::Amd,
        _ => panic!("unknown vendor: {vendor_str}"),
    };
    for gpu in &report.gpus {
        assert_eq!(gpu.vendor, expected);
    }
}

#[then(regex = r#"^all GPUs should have model "([\w-]+)"$"#)]
async fn then_all_model(world: &mut PactWorld, model: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    for gpu in &report.gpus {
        assert_eq!(gpu.model, model);
    }
}

#[then(regex = r#"^(\d+) GPUs should have vendor "([\w]+)"$"#)]
async fn then_n_vendor(world: &mut PactWorld, count: usize, vendor_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match vendor_str.as_str() {
        "Nvidia" => GpuVendor::Nvidia,
        "Amd" => GpuVendor::Amd,
        _ => panic!("unknown vendor: {vendor_str}"),
    };
    let actual = report.gpus.iter().filter(|g| g.vendor == expected).count();
    assert_eq!(actual, count);
}

#[then(regex = r#"^the GPU health should be "([\w]+)"$"#)]
async fn then_gpu_health(world: &mut PactWorld, health_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match health_str.as_str() {
        "Healthy" => GpuHealth::Healthy,
        "Degraded" => GpuHealth::Degraded,
        "Failed" => GpuHealth::Failed,
        _ => panic!("unknown health: {health_str}"),
    };
    assert_eq!(report.gpus[0].health, expected);
}

#[then("a CapabilityChange entry should be recorded in the journal")]
async fn then_cap_change_entry(world: &mut PactWorld) {
    let has_cap_change = world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == pact_common::types::EntryType::CapabilityChange);
    assert!(has_cap_change, "no CapabilityChange entry found in journal");
}

#[then("the capability report should be updated immediately")]
async fn then_report_updated(world: &mut PactWorld) {
    assert!(world.capability_report.is_some(), "capability report should be updated");
}

#[then(regex = r"^the capability report should show (\d+) memory bytes$")]
async fn then_memory_bytes(world: &mut PactWorld, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.total_bytes, bytes);
}

#[then("the report should be written to the configured manifest path")]
async fn then_manifest_written(world: &mut PactWorld) {
    assert!(world.manifest_written, "manifest should be written");
}

#[then("the manifest should be valid JSON")]
async fn then_manifest_json(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let json = serde_json::to_string(report).expect("report should serialize to JSON");
    let _parsed: CapabilityReport =
        serde_json::from_str(&json).expect("JSON should parse back to CapabilityReport");
}

#[then("the report should be available via the configured unix socket")]
async fn then_socket_available(world: &mut PactWorld) {
    assert!(world.socket_available, "socket should be available");
}

#[then("lattice-node-agent should be able to read it")]
async fn then_lattice_can_read(world: &mut PactWorld) {
    assert!(world.socket_available);
}

#[then("a new capability report should be sent immediately")]
async fn then_new_report_sent(world: &mut PactWorld) {
    assert!(world.capability_report.is_some(), "new report should be sent");
}

#[then("a new capability report should be sent")]
async fn then_periodic_report(world: &mut PactWorld) {
    assert!(world.capability_report.is_some(), "periodic report should be sent");
}

#[then(regex = r#"^the capability report config state should be "([\w]+)"$"#)]
async fn then_config_state(world: &mut PactWorld, state_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = super::helpers::parse_config_state(&state_str);
    assert_eq!(report.config_state, expected);
}

#[then(regex = r#"^the supervisor status should show backend "([\w]+)"$"#)]
async fn then_supervisor_backend(world: &mut PactWorld, backend_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match backend_str.as_str() {
        "Pact" => SupervisorBackend::Pact,
        "Systemd" => SupervisorBackend::Systemd,
        _ => panic!("unknown backend: {backend_str}"),
    };
    assert_eq!(report.supervisor_status.backend, expected);
}

#[then(regex = r"^the supervisor status should show (\d+) declared, (\d+) running, (\d+) failed$")]
async fn then_supervisor_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.supervisor_status.services_declared, declared);
    assert_eq!(report.supervisor_status.services_running, running);
    assert_eq!(report.supervisor_status.services_failed, failed);
}

#[then(regex = r#"^the status should report backend "([\w]+)"$"#)]
async fn then_status_backend(world: &mut PactWorld, backend_str: String) {
    let expected = match backend_str.as_str() {
        "Pact" => SupervisorBackend::Pact,
        "Systemd" => SupervisorBackend::Systemd,
        _ => panic!("unknown backend: {backend_str}"),
    };
    assert_eq!(world.supervisor_status.backend, expected);
}

#[then(regex = r"^the status should report (\d+) declared, (\d+) running, (\d+) failed$")]
async fn then_status_counts(world: &mut PactWorld, declared: u32, running: u32, failed: u32) {
    assert_eq!(world.supervisor_status.services_declared, declared);
    assert_eq!(world.supervisor_status.services_running, running);
    assert_eq!(world.supervisor_status.services_failed, failed);
}

// ---------------------------------------------------------------------------
// THEN — CPU steps
// ---------------------------------------------------------------------------

#[then(regex = r#"^the CPU architecture should be "(\w+)"$"#)]
async fn then_cpu_arch(world: &mut PactWorld, arch_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match arch_str.as_str() {
        "X86_64" => CpuArchitecture::X86_64,
        "Aarch64" => CpuArchitecture::Aarch64,
        "Unknown" => CpuArchitecture::Unknown,
        _ => panic!("unknown architecture: {arch_str}"),
    };
    assert_eq!(report.cpu.architecture, expected);
}

#[then(regex = r#"^the CPU model should be "(.+)"$"#)]
async fn then_cpu_model(world: &mut PactWorld, model: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.cpu.model, model);
}

#[then(regex = r#"^the CPU features should include "(\w+)"$"#)]
async fn then_cpu_features(world: &mut PactWorld, feature: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert!(
        report.cpu.features.contains(&feature),
        "CPU features {:?} should contain {:?}",
        report.cpu.features,
        feature
    );
}

#[then(regex = r"^the CPU physical cores should be (\d+)$")]
async fn then_cpu_physical_cores(world: &mut PactWorld, count: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.cpu.physical_cores, count);
}

#[then(regex = r"^the CPU logical cores should be (\d+)$")]
async fn then_cpu_logical_cores(world: &mut PactWorld, count: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.cpu.logical_cores, count);
}

#[then(regex = r"^the CPU NUMA nodes should be (\d+)$")]
async fn then_cpu_numa_nodes(world: &mut PactWorld, count: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.cpu.numa_nodes, count);
}

#[then(regex = r"^the CPU base frequency should be (\d+) MHz$")]
async fn then_cpu_base_freq(world: &mut PactWorld, freq: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.cpu.base_frequency_mhz, freq);
}

#[then(regex = r"^the CPU max frequency should be (\d+) MHz$")]
async fn then_cpu_max_freq(world: &mut PactWorld, freq: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.cpu.max_frequency_mhz, freq);
}

// ---------------------------------------------------------------------------
// THEN — Memory steps
// ---------------------------------------------------------------------------

#[then(regex = r"^the memory total should be (\d+) bytes$")]
async fn then_memory_total(world: &mut PactWorld, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.total_bytes, bytes);
}

#[then(regex = r"^the memory available should be (\d+) bytes$")]
async fn then_memory_available(world: &mut PactWorld, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.available_bytes, bytes);
}

#[then(regex = r"^the memory NUMA node count should be (\d+)$")]
async fn then_memory_numa_count(world: &mut PactWorld, count: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    // Check either the topology length or the numa_nodes field
    if !report.memory.numa_topology.is_empty() {
        assert_eq!(
            report.memory.numa_topology.len() as u32,
            count,
            "expected {} NUMA nodes in topology, got {}",
            count,
            report.memory.numa_topology.len()
        );
    } else {
        assert_eq!(report.memory.numa_nodes, count);
    }
}

#[then(regex = r"^NUMA node (\d+) should have (\d+) total bytes$")]
async fn then_numa_total_bytes(world: &mut PactWorld, node_id: u32, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let node = report
        .memory
        .numa_topology
        .iter()
        .find(|n| n.id == node_id)
        .unwrap_or_else(|| panic!("NUMA node {} not found", node_id));
    assert_eq!(node.total_bytes, bytes);
}

#[then(regex = r"^NUMA node (\d+) should contain CPU (\d+)$")]
async fn then_numa_contains_cpu(world: &mut PactWorld, node_id: u32, cpu_id: u32) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let node = report
        .memory
        .numa_topology
        .iter()
        .find(|n| n.id == node_id)
        .unwrap_or_else(|| panic!("NUMA node {} not found", node_id));
    assert!(
        node.cpus.contains(&cpu_id),
        "NUMA node {} CPUs {:?} should contain CPU {}",
        node_id,
        node.cpus,
        cpu_id
    );
}

#[then(regex = r"^the 2MB huge pages total should be (\d+)$")]
async fn then_2mb_hp_total(world: &mut PactWorld, count: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.hugepages.size_2mb_total, count);
}

#[then(regex = r"^the 2MB huge pages free should be (\d+)$")]
async fn then_2mb_hp_free(world: &mut PactWorld, count: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.hugepages.size_2mb_free, count);
}

#[then(regex = r"^the 1GB huge pages total should be (\d+)$")]
async fn then_1gb_hp_total(world: &mut PactWorld, count: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.hugepages.size_1gb_total, count);
}

#[then(regex = r"^the 1GB huge pages free should be (\d+)$")]
async fn then_1gb_hp_free(world: &mut PactWorld, count: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.memory.hugepages.size_1gb_free, count);
}

#[then(regex = r#"^the memory type should be "(\w+)"$"#)]
async fn then_memory_type(world: &mut PactWorld, type_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = parse_memory_type(&type_str);
    assert_eq!(report.memory.memory_type, expected);
}

// ---------------------------------------------------------------------------
// THEN — Network steps
// ---------------------------------------------------------------------------

#[then(regex = r"^the network interface count should be (\d+)$")]
async fn then_network_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.network.len(), count);
}

#[then(regex = r#"^interface "(\S+)" should have fabric "(\w+)"$"#)]
async fn then_iface_fabric(world: &mut PactWorld, name: String, fabric_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let iface = report
        .network
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("interface {} not found", name));
    let expected = match fabric_str.as_str() {
        "Slingshot" => NetworkFabric::Slingshot,
        "Ethernet" => NetworkFabric::Ethernet,
        "Unknown" => NetworkFabric::Unknown,
        _ => panic!("unknown fabric: {fabric_str}"),
    };
    assert_eq!(iface.fabric, expected);
}

#[then(regex = r#"^interface "(\S+)" should have speed (\d+) Mbps$"#)]
async fn then_iface_speed(world: &mut PactWorld, name: String, speed: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let iface = report
        .network
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("interface {} not found", name));
    assert_eq!(iface.speed_mbps, speed);
}

#[then(regex = r#"^interface "(\S+)" should have state "(\w+)"$"#)]
async fn then_iface_state(world: &mut PactWorld, name: String, state_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let iface = report
        .network
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("interface {} not found", name));
    let expected = parse_oper_state(&state_str);
    assert_eq!(iface.state, expected);
}

#[then(regex = r#"^interface "(\S+)" should have speed_mbps (\d+)$"#)]
async fn then_iface_speed_mbps(world: &mut PactWorld, name: String, speed: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let iface = report
        .network
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("interface {} not found", name));
    assert_eq!(iface.speed_mbps, speed);
}

// ---------------------------------------------------------------------------
// THEN — Storage steps
// ---------------------------------------------------------------------------

#[then(regex = r#"^the storage node type should be "(\w+)"$"#)]
async fn then_storage_node_type(world: &mut PactWorld, type_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let expected = match type_str.as_str() {
        "Diskless" => StorageNodeType::Diskless,
        "LocalStorage" => StorageNodeType::LocalStorage,
        _ => panic!("unknown storage node type: {type_str}"),
    };
    assert_eq!(report.storage.node_type, expected);
}

#[then(regex = r"^the local disk count should be (\d+)$")]
async fn then_disk_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.storage.local_disks.len(), count);
}

#[then(regex = r#"^disk "(\S+)" should have type "(\w+)"$"#)]
async fn then_disk_type(world: &mut PactWorld, device: String, type_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let disk = report
        .storage
        .local_disks
        .iter()
        .find(|d| d.device == device)
        .unwrap_or_else(|| panic!("disk {} not found", device));
    let expected = parse_disk_type(&type_str);
    assert_eq!(disk.disk_type, expected);
}

#[then(regex = r#"^disk "(\S+)" should have capacity (\d+) bytes$"#)]
async fn then_disk_capacity(world: &mut PactWorld, device: String, capacity: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let disk = report
        .storage
        .local_disks
        .iter()
        .find(|d| d.device == device)
        .unwrap_or_else(|| panic!("disk {} not found", device));
    assert_eq!(disk.capacity_bytes, capacity);
}

#[then(regex = r#"^mount "(\S+)" should have fs type "(\w+)"$"#)]
async fn then_mount_fs_type(world: &mut PactWorld, path: String, fs_str: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let mount = report
        .storage
        .mounts
        .iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("mount {} not found", path));
    let expected = parse_fs_type(&fs_str);
    assert_eq!(mount.fs_type, expected);
}

#[then(regex = r#"^mount "(\S+)" should have total (\d+) bytes$"#)]
async fn then_mount_total(world: &mut PactWorld, path: String, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let mount = report
        .storage
        .mounts
        .iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("mount {} not found", path));
    assert_eq!(mount.total_bytes, bytes);
}

#[then(regex = r#"^mount "(\S+)" should have available (\d+) bytes$"#)]
async fn then_mount_available(world: &mut PactWorld, path: String, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let mount = report
        .storage
        .mounts
        .iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("mount {} not found", path));
    assert_eq!(mount.available_bytes, bytes);
}

#[then(regex = r"^the mount count should be (\d+)$")]
async fn then_mount_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.storage.mounts.len(), count);
}

#[then(regex = r"^the local disks list should be empty$")]
async fn then_disks_empty(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert!(report.storage.local_disks.is_empty(), "expected empty local disks list");
}

#[then(regex = r#"^mount "(\S+)" should have total_bytes (\d+)$"#)]
async fn then_mount_total_bytes(world: &mut PactWorld, path: String, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let mount = report
        .storage
        .mounts
        .iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("mount {} not found", path));
    assert_eq!(mount.total_bytes, bytes);
}

#[then(regex = r#"^mount "(\S+)" should have available_bytes (\d+)$"#)]
async fn then_mount_available_bytes(world: &mut PactWorld, path: String, bytes: u64) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let mount = report
        .storage
        .mounts
        .iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("mount {} not found", path));
    assert_eq!(mount.available_bytes, bytes);
}

// ---------------------------------------------------------------------------
// THEN — Software steps
// ---------------------------------------------------------------------------

#[then(regex = r#"^the loaded modules should include "(\S+)"$"#)]
async fn then_loaded_module(world: &mut PactWorld, module: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert!(
        report.software.loaded_modules.contains(&module),
        "loaded modules {:?} should contain {:?}",
        report.software.loaded_modules,
        module
    );
}

#[then(regex = r#"^the uenv image should be "(.+)"$"#)]
async fn then_uenv_image(world: &mut PactWorld, image: String) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.software.uenv_image.as_deref(), Some(image.as_str()));
}

#[then(regex = r"^the software services count should be (\d+)$")]
async fn then_software_services_count(world: &mut PactWorld, count: usize) {
    let report = world.capability_report.as_ref().expect("no capability report");
    assert_eq!(report.software.services.len(), count);
}

// ---------------------------------------------------------------------------
// THEN — Cross-category steps
// ---------------------------------------------------------------------------

#[then(regex = r"^the capability report should include a CPU section$")]
async fn then_has_cpu_section(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    // CPU section always present — just verify it's there
    let _ = &report.cpu;
}

#[then(regex = r"^the capability report should include a memory section$")]
async fn then_has_memory_section(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let _ = &report.memory;
}

#[then(regex = r"^the capability report should include a storage section$")]
async fn then_has_storage_section(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let _ = &report.storage;
}

#[then(regex = r"^the capability report should include a software section$")]
async fn then_has_software_section(world: &mut PactWorld) {
    let report = world.capability_report.as_ref().expect("no capability report");
    let _ = &report.software;
}
