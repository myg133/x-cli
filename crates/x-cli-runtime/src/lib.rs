//! x-cli-runtime: JSON-RPC over stdio 传输层 + HTTP 客户端

#![warn(missing_docs)]

pub mod auth;
pub mod http;
pub mod transport;
pub mod workflow_executor;

pub use auth::build_auth_profile;
pub use http::{HttpCaller, AuthProfile};
pub use transport::{serve, serve_stdio};
pub use workflow_executor::WorkflowExecutor;
