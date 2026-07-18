//! x-cli-core: IR 数据模型、OpenAPI 解析、协议 schema
//!
//! 这是整个 x-cli 的中间表示层。后面 emitter 和 runtime 都基于这里的类型工作。

#![warn(missing_docs)]

pub mod ir;
pub mod openapi;
pub mod protocol;
pub mod error;

pub use error::{Error, Result};
pub use ir::{ApiSpec, Domain, Endpoint, Param, ParamLocation, RequestBody, Response, HttpMethod};
pub use openapi::{parse_openapi, parse_openapi_str};
pub use protocol::{RpcRequest, RpcResponse, RpcError, RpcMethod};
