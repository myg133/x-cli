//! OpenAPI 解析
//!
//! 把 `oas3::OpenApiV3Spec` 转换为我们的 IR `ApiSpec`。
//! A 阶段：扁平化所有 path + method，归类到 tag 域，参数/请求体/响应只取第一个 schema。
//! B 阶段：$ref 通过 `oas3::ObjectOrReference::resolve(spec)` 解析为 ObjectSchema，
//!          递归构建 ResolvedSchema，循环引用通过路径栈检测并标记 `recursive: true`。

use crate::error::{Error, Result};
use crate::ir::{
    ApiSpec, Domain, Endpoint, HttpMethod, Param, ParamLocation, RequestBody, ResolvedSchema,
    Response, SchemaKind, SchemaRef,
};
use oas3::spec::{
    ObjectOrReference, ObjectSchema, Operation, ParameterIn, PathItem, SchemaType, SchemaTypeSet,
    Server,
};
use oas3::{OpenApiV3Spec, Spec as OasSpec};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 从文件读取并解析 OpenAPI 3 文档
pub fn parse_openapi<P: AsRef<Path>>(path: P) -> Result<ApiSpec> {
    let content = std::fs::read_to_string(path.as_ref())?;
    parse_openapi_str(&content)
}

/// 从 YAML 字符串解析 OpenAPI 3 文档
pub fn parse_openapi_str(yaml: &str) -> Result<ApiSpec> {
    let spec: OpenApiV3Spec = oas3::from_yaml(yaml).map_err(|e| Error::OpenApiParse(e.to_string()))?;
    Ok(convert(&spec))
}

fn convert(spec: &OasSpec) -> ApiSpec {
    let title = spec.info.title.clone();
    let version = spec.info.version.clone();
    let description = spec.info.description.clone().filter(|d| !d.is_empty());
    let base_url = first_server_url(&spec.servers);

    let mut ctx = ResolveCtx::new();
    // 注：不要在这里预填 components.schemas，否则第一次 resolve 会命中"any"占位。
    // cache 只在 schema_from_object 完成后写入。

    let mut endpoints: BTreeMap<String, Endpoint> = BTreeMap::new();
    let mut by_tag: BTreeMap<String, Vec<String>> = BTreeMap::new();

    if let Some(paths) = &spec.paths {
        for (path, path_item) in paths {
            let methods: [(HttpMethod, &Option<Operation>); 7] = [
                (HttpMethod::Get, &path_item.get),
                (HttpMethod::Post, &path_item.post),
                (HttpMethod::Put, &path_item.put),
                (HttpMethod::Patch, &path_item.patch),
                (HttpMethod::Delete, &path_item.delete),
                (HttpMethod::Head, &path_item.head),
                (HttpMethod::Options, &path_item.options),
            ];

            for (method, op_opt) in methods {
                let Some(op) = op_opt else { continue };
                let domain = pick_domain(op, path);
                let id = make_endpoint_id(&domain, method, path);

                let endpoint = Endpoint {
                    id: id.clone(),
                    domain: domain.clone(),
                    method,
                    path: path.clone(),
                    operation_id: op.operation_id.clone(),
                    summary: op.summary.clone(),
                    description: op.description.clone(),
                    tags: op.tags.clone(),
                    params: convert_params(&op.parameters, spec, &mut ctx),
                    request_body: convert_request_body(op.request_body.as_ref(), spec, &mut ctx),
                    responses: convert_responses(op.responses.as_ref(), spec, &mut ctx),
                    deprecated: op.deprecated.unwrap_or(false),
                };

                endpoints.insert(id.clone(), endpoint);
                by_tag.entry(domain).or_default().push(id);
            }
        }
    }

    let domains = by_tag
        .into_iter()
        .map(|(name, endpoint_ids)| Domain {
            name,
            description: None,
            endpoint_ids,
        })
        .collect();

    ApiSpec {
        title,
        version,
        description,
        base_url,
        domains,
        endpoints,
    }
}

/// 解析上下文：缓存 + 循环检测
struct ResolveCtx {
    /// 解析过的 schema 缓存（按名）
    cache: BTreeMap<String, SchemaRef>,
    /// 正在解析链上的 schema 名（循环检测）
    in_progress: BTreeSet<String>,
}

impl ResolveCtx {
    fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
            in_progress: BTreeSet::new(),
        }
    }
}

fn first_server_url(servers: &[Server]) -> Option<String> {
    let url = servers.first()?.url.clone();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

/// 选 domain：优先用第一个 tag；都没有用 path 第一段；最终兜底 "default"
fn pick_domain(op: &Operation, path: &str) -> String {
    if let Some(t) = op.tags.first() {
        return t.clone();
    }
    path.trim_start_matches('/')
        .split('/')
        .next()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .unwrap_or_else(|| "default".to_string())
}

fn make_endpoint_id(domain: &str, method: HttpMethod, path: &str) -> String {
    let sanitized_path = path
        .trim_start_matches('/')
        .replace('/', "_")
        .replace('{', "")
        .replace('}', "");
    let method_str = match method {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
        HttpMethod::Head => "head",
        HttpMethod::Options => "options",
    };
    format!("{domain}__{method_str}__{sanitized_path}")
}

fn convert_params(
    params: &[ObjectOrReference<oas3::spec::Parameter>],
    spec: &OasSpec,
    ctx: &mut ResolveCtx,
) -> Vec<Param> {
    params
        .iter()
        .filter_map(|oor| match oor {
            ObjectOrReference::Object(p) => Some(convert_one_param(p, spec, ctx)),
            ObjectOrReference::Ref { ref_path } => {
                // 通过 resolve 拿到 Parameter，然后转
                match oor.resolve(spec) {
                    Ok(p) => Some(convert_one_param(&p, spec, ctx)),
                    Err(e) => {
                        tracing::warn!(error = %e, ref = %ref_path, "skip $ref parameter");
                        None
                    }
                }
            }
        })
        .collect()
}

fn convert_one_param(p: &oas3::spec::Parameter, spec: &OasSpec, ctx: &mut ResolveCtx) -> Param {
    let location = match p.location {
        ParameterIn::Query => ParamLocation::Query,
        ParameterIn::Path => ParamLocation::Path,
        ParameterIn::Header => ParamLocation::Header,
        ParameterIn::Cookie => ParamLocation::Cookie,
    };
    Param {
        name: p.name.clone(),
        location,
        required: p.required.unwrap_or(false),
        description: p.description.clone(),
        schema: resolve_schema_ref(&p.schema, spec, ctx),
    }
}

fn convert_request_body(
    rb: Option<&ObjectOrReference<oas3::spec::RequestBody>>,
    spec: &OasSpec,
    ctx: &mut ResolveCtx,
) -> Option<RequestBody> {
    let oor = rb?;
    let rb = match oor.resolve(spec) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "skip $ref request body");
            return None;
        }
    };
    let (content_type, media) = rb.content.iter().next()?;
    Some(RequestBody {
        required: rb.required.unwrap_or(false),
        content_type: content_type.clone(),
        schema: resolve_schema_ref(&media.schema, spec, ctx),
    })
}

fn convert_responses(
    responses: Option<&BTreeMap<String, ObjectOrReference<oas3::spec::Response>>>,
    spec: &OasSpec,
    ctx: &mut ResolveCtx,
) -> Vec<Response> {
    let Some(responses) = responses else { return Vec::new() };
    let mut out: Vec<Response> = responses
        .iter()
        .filter_map(|(status_str, oor)| {
            let resp = match oor.resolve(spec) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, status = %status_str, "skip $ref response");
                    return None;
                }
            };
            let status: u16 = status_str.parse().unwrap_or(0);
            let (content_type, schema) = resp
                .content
                .iter()
                .next()
                .map(|(ct, m)| (Some(ct.clone()), Some(resolve_schema_ref(&m.schema, spec, ctx))))
                .unwrap_or((None, None));
            Some(Response {
                status,
                description: resp.description.clone(),
                content_type,
                schema,
            })
        })
        .collect();
    out.sort_by_key(|r| r.status);
    out
}

/// 核心：把 oas3 schema 转换为我们 IR 的 SchemaRef，递归解析 $ref
///
/// 循环检测：如果 `$ref` 指向的 schema 名字已经在当前解析链上，标记 `recursive: true`
/// 不再深入。
fn resolve_schema_ref(
    oor: &Option<ObjectOrReference<ObjectSchema>>,
    spec: &OasSpec,
    ctx: &mut ResolveCtx,
) -> SchemaRef {
    let Some(oor) = oor else {
        return SchemaRef::any();
    };

    // 1. 如果是 $ref，提取 schema 名字做缓存/循环检测
    if let ObjectOrReference::Ref { ref_path } = oor {
        if let Some(name) = ref_schema_name(ref_path) {
            // 缓存命中：直接复用
            if let Some(cached) = ctx.cache.get(&name) {
                return cached.clone();
            }
            // 循环引用：标记 recursive: true 不再深入
            if ctx.in_progress.contains(&name) {
                return SchemaRef {
                    name: name.clone(),
                    description: Some(format!("recursive: {ref_path}")),
                    json_schema: serde_json::json!({ "$ref": ref_path, "recursive": true }),
                    resolved: Some(Box::new(ResolvedSchema {
                        kind: SchemaKind::Object,
                        properties: BTreeMap::new(),
                        required: Vec::new(),
                        items: None,
                        recursive: true,
                    })),
                };
            }
        }
    }

    // 2. 解析为 ObjectSchema
    let obj = match oor.resolve(spec) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "skip unresolved $ref schema");
            return SchemaRef::any();
        }
    };

    // 3. 进入解析链
    let name_hint = ref_schema_name_for_oor(oor, &obj);
    let prev_in_progress = if let Some(ref n) = name_hint {
        ctx.in_progress.insert(n.clone());
        Some(n.clone())
    } else {
        None
    };

    let result = schema_from_object(&obj, spec, ctx);

    // 4. 出解析链 + 写缓存
    if let Some(n) = prev_in_progress {
        ctx.in_progress.remove(&n);
        ctx.cache.insert(n, result.clone());
    }

    result
}

fn schema_from_object(obj: &ObjectSchema, spec: &OasSpec, ctx: &mut ResolveCtx) -> SchemaRef {
    let name = obj
        .title
        .clone()
        .or_else(|| Some(type_set_label(&obj.schema_type)))
        .unwrap_or_else(|| "any".to_string());
    let description = obj.description.clone();
    let json_schema = serde_json::to_value(obj).unwrap_or(serde_json::json!({}));

    // resolved 树：按 schema 类型分四种情况
    let kind = infer_kind(obj);
    let (properties, required, items) = match kind {
        SchemaKind::Object => {
            let mut props = BTreeMap::new();
            for (pname, poor) in &obj.properties {
                let wrapped = Some(poor.clone());
                props.insert(pname.clone(), resolve_schema_ref(&wrapped, spec, ctx));
            }
            (props, obj.required.clone(), None)
        }
        SchemaKind::Array => {
            let items = obj
                .items
                .as_ref()
                .map(|i| Box::new(resolve_schema_ref(&Some(i.as_ref().clone()), spec, ctx)));
            (BTreeMap::new(), Vec::new(), items)
        }
        _ => (BTreeMap::new(), Vec::new(), None),
    };

    let resolved = Box::new(ResolvedSchema {
        kind,
        properties,
        required,
        items,
        recursive: false,
    });

    SchemaRef {
        name,
        description,
        json_schema,
        resolved: Some(resolved),
    }
}

/// 从 $ref path 提取 schema 名（只处理 `#/components/schemas/<Name>`）
fn ref_schema_name(ref_path: &str) -> Option<String> {
    let prefix = "#/components/schemas/";
    if let Some(rest) = ref_path.strip_prefix(prefix) {
        // 名字里可能有 / 之类的复杂情况，简单按第一段取
        let name = rest.split('/').next()?.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// 给 ref + 解析结果命名（用于循环检测的 in_progress key）
fn ref_schema_name_for_oor(
    oor: &ObjectOrReference<ObjectSchema>,
    obj: &ObjectSchema,
) -> Option<String> {
    if let ObjectOrReference::Ref { ref_path } = oor {
        if let Some(n) = ref_schema_name(ref_path) {
            return Some(n);
        }
    }
    // 内联 schema 用 title 当名字
    obj.title.clone()
}

fn infer_kind(obj: &ObjectSchema) -> SchemaKind {
    let Some(ts) = &obj.schema_type else { return SchemaKind::Any };
    match ts {
        SchemaTypeSet::Single(t) => single_kind(*t),
        SchemaTypeSet::Multiple(set) => {
            // 多类型时优先 Object
            if set.iter().any(|t| matches!(t, SchemaType::Object)) {
                SchemaKind::Object
            } else if set.iter().any(|t| matches!(t, SchemaType::Array)) {
                SchemaKind::Array
            } else {
                SchemaKind::Scalar
            }
        }
    }
}

fn single_kind(t: SchemaType) -> SchemaKind {
    match t {
        SchemaType::Object => SchemaKind::Object,
        SchemaType::Array => SchemaKind::Array,
        _ => SchemaKind::Scalar,
    }
}

fn type_set_label(ts: &Option<SchemaTypeSet>) -> String {
    let Some(ts) = ts else { return "any".to_string() };
    match ts {
        SchemaTypeSet::Single(t) => format!("{:?}", t).to_lowercase(),
        SchemaTypeSet::Multiple(ts) => ts
            .iter()
            .map(|t| format!("{:?}", t).to_lowercase())
            .collect::<Vec<_>>()
            .join("|"),
    }
}

#[allow(dead_code)]
fn _path_item_marker(_p: &PathItem) {} // 防止 PathItem import 被 unused
