//! x-cli-core: IR 数据模型、OpenAPI 解析、协议 schema
//!
//! 这是整个 x-cli 的中间表示层。后面 emitter 和 runtime 都基于这里的类型工作。

#![warn(missing_docs)]

pub mod error;
pub mod ir;
pub mod openapi;
pub mod protocol;
pub mod workflow;

pub use error::{Error, Result};
pub use ir::{
    ApiSpec, Domain, Endpoint, HttpMethod, InputRef, Param, ParamLocation, RequestBody,
    ResolvedSchema, Response, SchemaKind, SchemaRef, StepInputs, Workflow, WorkflowInput,
    WorkflowStep,
};
pub use openapi::{parse_openapi, parse_openapi_str, parse_openapi_str_json};
pub use protocol::{RpcError, RpcMethod, RpcRequest, RpcResponse};
pub use workflow::{parse_workflow, parse_workflow_str};
