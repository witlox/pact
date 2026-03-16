//! gRPC authentication interceptor for journal services (F-C1).
//!
//! Validates that all gRPC requests carry a Bearer token in the
//! `authorization` metadata header. Health and metrics endpoints are
//! served on a separate axum server and are not affected.

use tonic::Status;

/// Interceptor that requires a Bearer token in the `authorization` header.
///
/// Rejects requests without a valid authorization header with
/// `Status::unauthenticated`.
pub fn auth_interceptor(req: tonic::Request<()>) -> Result<tonic::Request<()>, Status> {
    match req.metadata().get("authorization") {
        Some(token) => {
            let token_str = token
                .to_str()
                .map_err(|_| Status::unauthenticated("invalid authorization header"))?;
            if !token_str.starts_with("Bearer ") {
                return Err(Status::unauthenticated("expected Bearer token"));
            }
            Ok(req)
        }
        None => Err(Status::unauthenticated("missing authorization header")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    #[test]
    fn accepts_valid_bearer_token() {
        let mut req = Request::new(());
        req.metadata_mut().insert("authorization", "Bearer test-token-123".parse().unwrap());
        assert!(auth_interceptor(req).is_ok());
    }

    #[test]
    fn rejects_missing_authorization() {
        let req = Request::new(());
        let err = auth_interceptor(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("missing authorization header"));
    }

    #[test]
    fn rejects_non_bearer_token() {
        let mut req = Request::new(());
        req.metadata_mut().insert("authorization", "Basic dXNlcjpwYXNz".parse().unwrap());
        let err = auth_interceptor(req).unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("expected Bearer token"));
    }
}
