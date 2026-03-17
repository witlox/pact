#![allow(clippy::needless_pass_by_value)]
//! Identity mapping steps — wired to identity module and UidMap.

use cucumber::{given, then, when};
use pact_agent::identity::IdentityManager;
use pact_common::types::{GroupEntry, IdentityMode, OrgIndex, UidMap};

use crate::PactWorld;

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r#"^identity_mode is "(on-demand|pre-provisioned)"$"#)]
fn given_identity_mode(world: &mut PactWorld, mode: String) {
    world.identity_mode = match mode.as_str() {
        "on-demand" => IdentityMode::OnDemand,
        "pre-provisioned" => IdentityMode::PreProvisioned,
        _ => panic!("unknown identity mode: {mode}"),
    };
}

#[given(regex = r#"^org "(.+)" has org_index (\d+) with stride (\d+) and base_uid (\d+)$"#)]
fn given_org_config(world: &mut PactWorld, org: String, index: String, stride: String, base: String) {
    let mut map = world.uid_map.take().unwrap_or_default();
    map.stride = stride.parse().unwrap();
    map.base_uid = base.parse().unwrap();
    map.base_gid = base.parse().unwrap();
    map.org_indices.push(OrgIndex {
        org,
        index: index.parse().unwrap(),
    });
    world.uid_map = Some(map);
}

#[given(regex = r#"^no UidEntry exists for subject "(.+)"$"#)]
fn given_no_uid_entry(world: &mut PactWorld, subject: String) {
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    assert!(!map.users.contains_key(&subject));
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^"(.+)" authenticates via OIDC$"#)]
fn when_authenticates(world: &mut PactWorld, subject: String) {
    world.last_auth_subject = Some(subject);
}

#[when(regex = r#"^a new subject "(.+)" is assigned a UID$"#)]
fn when_assign_uid(world: &mut PactWorld, subject: String) {
    let map = world.uid_map.as_mut().expect("UidMap not initialized");
    let username = subject.split('@').next().unwrap_or("user");
    let org = map.org_indices.first().map_or_else(|| "local".into(), |o| o.org.clone());
    match map.assign_uid(&subject, username, &org, &format!("/users/{username}"), "/bin/bash") {
        Ok(entry) => world.last_assigned_uid = Some(entry.uid),
        Err(e) => world.last_error = Some(e),
    }
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r"^a UidEntry should be created with uid in range (\d+)-(\d+)$")]
fn then_uid_in_range(world: &mut PactWorld, min: String, max: String) {
    let min: u32 = min.parse().unwrap();
    let max: u32 = max.parse().unwrap();
    let map = world.uid_map.as_mut().expect("UidMap not initialized");
    let subject = world.last_auth_subject.as_ref().expect("no auth subject");
    let username = subject.split('@').next().unwrap_or("user");
    let org = map.org_indices.first().map_or_else(|| "local".into(), |o| o.org.clone());
    let entry = map
        .assign_uid(subject, username, &org, &format!("/users/{username}"), "/bin/bash")
        .unwrap();
    assert!(
        entry.uid >= min && entry.uid <= max,
        "UID {} not in range {}-{}",
        entry.uid,
        min,
        max
    );
}

#[then(regex = r"^the assigned UID should be (\d+)$")]
fn then_assigned_uid(world: &mut PactWorld, expected: String) {
    let expected: u32 = expected.parse().unwrap();
    assert_eq!(
        world.last_assigned_uid,
        Some(expected),
        "expected UID {expected}"
    );
}

#[then(regex = r#"^the assignment should fail with "(.+)"$"#)]
fn then_assignment_fails(world: &mut PactWorld, expected_msg: String) {
    let err = world.last_error.as_ref().expect("expected an error");
    let err_str = err.to_string();
    assert!(
        err_str.contains(&expected_msg),
        "error '{err_str}' does not contain '{expected_msg}'"
    );
}
