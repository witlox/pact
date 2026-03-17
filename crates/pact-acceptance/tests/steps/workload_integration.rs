#![allow(clippy::needless_pass_by_value)]
//! Workload integration steps — namespace handoff and mount refcounting.

use cucumber::{given, then, when};
use hpc_node::namespace::{NamespaceProvider, NamespaceRequest, NamespaceType};
use pact_agent::handoff::{HandoffServer, MountRefManager};

use crate::PactWorld;

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given("lattice-node-agent is a supervised service")]
fn given_lattice_supervised(_world: &mut PactWorld) {
    // lattice-node-agent is in the service list — accepted
}

#[given(regex = r#"^no mount exists for uenv image "(.+)"$"#)]
fn given_no_mount(world: &mut PactWorld, image: String) {
    let mgr = world.mount_manager.get_or_insert_with(|| MountRefManager::new("/run/pact/uenv", 60));
    assert!(mgr.refcount(&image).is_none());
}

#[given(regex = r#"^"(.+)" is mounted with refcount (\d+)"#)]
fn given_mounted_with_refcount(world: &mut PactWorld, image: String, count: String) {
    let count: u32 = count.parse().unwrap();
    let mgr = world.mount_manager.get_or_insert_with(|| MountRefManager::new("/run/pact/uenv", 60));
    for _ in 0..count {
        mgr.acquire(&image).unwrap();
    }
    assert_eq!(mgr.refcount(&image), Some(count));
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^allocation "(.+)" requests uenv "(.+)"$"#)]
fn when_alloc_requests_uenv(world: &mut PactWorld, _alloc: String, image: String) {
    let mgr = world.mount_manager.get_or_insert_with(|| MountRefManager::new("/run/pact/uenv", 60));
    mgr.acquire(&image).unwrap();
}

#[when(regex = r#"^allocation "(.+)" releases$"#)]
fn when_alloc_releases(world: &mut PactWorld, _alloc: String) {
    // Release the first mounted image
    if let Some(ref mut mgr) = world.mount_manager {
        let images: Vec<String> = mgr.states().iter().map(|s| s.image_path.clone()).collect();
        if let Some(img) = images.first() {
            mgr.release(img);
        }
    }
}

#[when("the last allocation releases")]
fn when_last_alloc_releases(world: &mut PactWorld) {
    if let Some(ref mut mgr) = world.mount_manager {
        let images: Vec<String> =
            mgr.states().iter().filter(|s| s.refcount > 0).map(|s| s.image_path.clone()).collect();
        if let Some(img) = images.first() {
            mgr.release(img);
        }
    }
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r"^MountRef refcount should (?:be|increase to|decrease to) (\d+)$")]
fn then_refcount(world: &mut PactWorld, expected: String) {
    let expected: u32 = expected.parse().unwrap();
    let mgr = world.mount_manager.as_ref().expect("no mount manager");
    let binding = mgr.states();
    let state = binding.first().expect("no mounts tracked");
    assert_eq!(state.refcount, expected, "expected refcount {expected}, got {}", state.refcount);
}

#[then(regex = r"^no new `SquashFS` mount should occur$")]
fn then_no_new_mount(world: &mut PactWorld) {
    let mgr = world.mount_manager.as_ref().expect("no mount manager");
    assert_eq!(mgr.mount_count(), 1, "should still be 1 mount");
}

#[then("a cache hold timer should start")]
fn then_hold_timer_started(world: &mut PactWorld) {
    let mgr = world.mount_manager.as_ref().expect("no mount manager");
    let binding = mgr.states();
    let state = binding.first().expect("no mounts");
    assert_eq!(state.refcount, 0);
    assert!(state.hold_start.is_some());
}

#[then("the mount should not be unmounted yet")]
fn then_mount_still_exists(world: &mut PactWorld) {
    let mgr = world.mount_manager.as_ref().expect("no mount manager");
    assert_eq!(mgr.mount_count(), 1);
}
