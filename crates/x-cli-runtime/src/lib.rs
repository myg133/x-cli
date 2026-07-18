//! x-cli-runtime: JSON-RPC over stdio 传输层 + HTTP 客户端

#![warn(missing_docs)]

pub mod http;
pub mod transport;

pub use http::{HttpCaller, AuthProfile};
pub use transport::{serve, serve_stdio};
