//! `pact-mcp` — MCP (Model Context Protocol) server for AI agent tool-use.
//!
//! Wraps pact gRPC APIs as MCP tools for Claude Code-style AI agents.
//! Authenticates as `pact-service-ai` principal with scoped permissions.
//!
//! See `docs/architecture/agentic-api.md` for design documentation.

pub mod connected;
pub mod protocol;
pub mod tools;
