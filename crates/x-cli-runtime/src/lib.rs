//! x-cli-runtime: JSON-RPC over stdio 传输层 + HTTP 客户端 + Session auth

#![warn(missing_docs)]

pub mod http;
pub mod mcp_transport;
pub mod session;
pub mod transport;
pub mod workflow_executor;

pub use http::HttpCaller;
pub use mcp_transport::{serve_mcp, serve_mcp_stdio};
pub use session::Session;
pub use transport::{serve, serve_stdio};
pub use workflow_executor::WorkflowExecutor;
