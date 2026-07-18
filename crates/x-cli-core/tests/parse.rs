//! OpenAPI 解析的回归测试
//!
//! 防止 IR 字段、endpoint id、domain 划分在后续重构里悄悄坏掉。

use x_cli_core::parse_openapi_str;

const PETSTORE: &str = include_str!("fixtures/petstore.yaml");
const HTTPBIN: &str = include_str!("fixtures/httpbin.yaml");

#[test]
fn petstore_basic_metadata() {
    let spec = parse_openapi_str(PETSTORE).expect("parse petstore");
    assert_eq!(spec.title, "Pet Store API");
    assert_eq!(spec.version, "1.0.0");
    assert_eq!(spec.base_url.as_deref(), Some("https://petstore.example.com/v1"));
    assert!(
        spec.description.as_deref().unwrap().contains("极简示例"),
        "description should contain 极简示例"
    );
}

#[test]
fn petstore_endpoint_count_and_id_pattern() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    // 4 个 path × 多个 method = 5 个接口
    assert_eq!(spec.endpoints.len(), 5);

    // 每个 id 都符合 `<domain>__<method>__<path>` 模式
    for id in spec.endpoints.keys() {
        let s = id.as_str();
        assert!(s.contains("__"), "id should contain separator: {s}");
        assert!(!s.contains('{') && !s.contains('}'), "id should be path-flattened: {s}");
    }

    // 关键 endpoint 存在
    assert!(spec.endpoints.contains_key("pet__get__pets_petId"));
    assert!(spec.endpoints.contains_key("pet__post__pets"));
    assert!(spec.endpoints.contains_key("store__post__store_orders"));
    assert!(spec.endpoints.contains_key("store__get__store_orders_orderId"));
    assert!(spec.endpoints.contains_key("pet__get__pets"));
}

#[test]
fn petstore_domains() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let pet = spec.domains.iter().find(|d| d.name == "pet").expect("pet domain");
    assert_eq!(pet.endpoint_ids.len(), 3);
    let store = spec.domains.iter().find(|d| d.name == "store").expect("store domain");
    assert_eq!(store.endpoint_ids.len(), 2);
}

#[test]
fn petstore_path_param_substitution_target() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let ep = spec
        .endpoints
        .get("pet__get__pets_petId")
        .expect("getPet endpoint");
    assert_eq!(ep.path, "/pets/{petId}");
    let path_params: Vec<_> = ep
        .params
        .iter()
        .filter(|p| matches!(p.location, x_cli_core::ParamLocation::Path))
        .collect();
    assert_eq!(path_params.len(), 1);
    assert_eq!(path_params[0].name, "petId");
    assert!(path_params[0].required);
}

#[test]
fn petstore_request_body() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let ep = spec
        .endpoints
        .get("pet__post__pets")
        .expect("createPet endpoint");
    let rb = ep.request_body.as_ref().expect("createPet has body");
    assert!(rb.required);
    assert_eq!(rb.content_type, "application/json");
}

#[test]
fn petstore_responses_parsed() {
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let ep = spec
        .endpoints
        .get("pet__get__pets_petId")
        .expect("getPet");
    let statuses: Vec<u16> = ep.responses.iter().map(|r| r.status).collect();
    assert!(statuses.contains(&200));
    assert!(statuses.contains(&404));
}

#[test]
fn httpbin_basic() {
    let spec = parse_openapi_str(HTTPBIN).expect("parse httpbin");
    assert_eq!(spec.endpoints.len(), 2);
    assert_eq!(spec.base_url.as_deref(), Some("https://httpbin.org"));
    // path param
    let anything = spec
        .endpoints
        .values()
        .find(|e| e.path == "/anything/{path}")
        .expect("anything endpoint");
    assert!(anything
        .params
        .iter()
        .any(|p| matches!(p.location, x_cli_core::ParamLocation::Path) && p.name == "path"));
}
