//! Protobuf-generated types for pact gRPC services.
//!
//! Generated code is exempt from clippy lints.

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod config {
    tonic::include_proto!("pact.config");
}

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod shell {
    tonic::include_proto!("pact.shell");
}

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod capability {
    tonic::include_proto!("pact.capability");
}

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod policy {
    tonic::include_proto!("pact.policy");
}

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod stream {
    tonic::include_proto!("pact.stream");
}

#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod journal {
    tonic::include_proto!("pact.journal");
}
