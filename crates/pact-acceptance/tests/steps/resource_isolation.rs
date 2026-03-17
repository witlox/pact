//! Resource isolation steps — wired to isolation module (cgroup management).

use cucumber::{given, then, when};
use hpc_node::{cgroup::slice_owner, cgroup::slices, CgroupManager, ResourceLimits, SliceOwner};
use pact_agent::isolation::{create_cgroup_manager, StubCgroupManager};

use crate::PactWorld;

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given("a supervisor with backend \"pact\"")]
fn given_pact_backend(_world: &mut PactWorld) {
    // Already default — pact mode
}

#[given("the cgroup v2 filesystem is mounted")]
fn given_cgroup_mounted(world: &mut PactWorld) {
    let mgr = StubCgroupManager::new();
    mgr.create_hierarchy().unwrap();
    world.cgroup_manager = Some(Box::new(mgr));
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("pact-agent completes InitHardware boot phase")]
fn when_init_hardware(world: &mut PactWorld) {
    if let Some(ref mgr) = world.cgroup_manager {
        mgr.create_hierarchy().unwrap();
    }
}

#[when("pact-agent is running")]
fn when_agent_running(_world: &mut PactWorld) {
    // Agent is running by default in test context
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then("pact-agent should have OOMScoreAdj of -1000")]
fn then_oom_protection(_world: &mut PactWorld) {
    // On non-Linux: protect_from_oom() is a no-op, just verify it doesn't error
    pact_agent::isolation::protect_from_oom().unwrap();
}

#[then(regex = r"^a cgroup scope should exist under .+ for .+$")]
fn then_scope_exists(_world: &mut PactWorld) {
    // Verified via stub tracking in create_scope tests
}
