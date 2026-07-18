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

// ─────────────── B 阶段：$ref 解析 ───────────────

#[test]
fn petstore_request_body_resolves_pet_schema() {
    use x_cli_core::SchemaKind;
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let ep = spec
        .endpoints
        .get("pet__post__pets")
        .expect("createPet endpoint");
    let rb = ep.request_body.as_ref().expect("has body");
    let resolved = rb.schema.resolved.as_ref().expect("resolved tree");
    assert_eq!(resolved.kind, SchemaKind::Object);
    // Pet 的三个字段必须都解析出来
    assert!(resolved.properties.contains_key("id"));
    assert!(resolved.properties.contains_key("name"));
    assert!(resolved.properties.contains_key("tag"));
    // name 是 required
    assert!(resolved.required.contains(&"name".to_string()));
    // 不是 recursive
    assert!(!resolved.recursive);
}

#[test]
fn petstore_response_schema_also_resolves() {
    use x_cli_core::SchemaKind;
    let spec = parse_openapi_str(PETSTORE).expect("parse");
    let ep = spec
        .endpoints
        .get("pet__get__pets_petId")
        .expect("getPet");
    let resp_200 = ep
        .responses
        .iter()
        .find(|r| r.status == 200)
        .expect("200 response");
    let schema = resp_200.schema.as_ref().expect("schema");
    let resolved = schema.resolved.as_ref().expect("resolved");
    assert_eq!(resolved.kind, SchemaKind::Object);
    assert!(resolved.properties.contains_key("name"));
}

#[test]
fn recursive_schema_does_not_stack_overflow() {
    // 自引用 schema：Tree { value, children: [Tree] }
    let yaml = r#"
openapi: 3.0.3
info:
  title: Tree
  version: 1.0.0
paths:
  /tree:
    get:
      tags: [tree]
      operationId: getTree
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Tree'
components:
  schemas:
    Tree:
      type: object
      title: Tree
      required: [value]
      properties:
        value:
          type: string
        children:
          type: array
          items:
            $ref: '#/components/schemas/Tree'
"#;
    let spec = parse_openapi_str(yaml).expect("parse");
    let ep = spec.endpoints.get("tree__get__tree").expect("getTree");
    let r = ep
        .responses
        .iter()
        .find(|r| r.status == 200)
        .expect("200");
    let schema = r.schema.as_ref().expect("schema");
    let resolved = schema.resolved.as_ref().expect("resolved");
    assert_eq!(resolved.kind, x_cli_core::SchemaKind::Object);
    // 顶层有 value + children
    assert!(resolved.properties.contains_key("value"));
    assert!(resolved.properties.contains_key("children"));
    // children 数组的 items 应该是 recursive
    let children = resolved.properties.get("children").expect("children");
    let children_resolved = children.resolved.as_ref().expect("children resolved");
    assert_eq!(children_resolved.kind, x_cli_core::SchemaKind::Array);
    let items = children_resolved.items.as_ref().expect("items");
    let items_resolved = items.resolved.as_ref().expect("items resolved");
    assert!(items_resolved.recursive, "items should be marked recursive");
}

#[test]
fn self_referential_object_schema_marked_recursive() {
    // 直接自引用：A { refToA: A }
    let yaml = r#"
openapi: 3.0.3
info:
  title: A
  version: 1.0.0
paths:
  /a:
    get:
      tags: [a]
      operationId: getA
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/A'
components:
  schemas:
    A:
      type: object
      title: A
      properties:
        refToA:
          $ref: '#/components/schemas/A'
"#;
    let spec = parse_openapi_str(yaml).expect("parse");
    let ep = spec.endpoints.get("a__get__a").expect("getA");
    let r = ep.responses.iter().find(|r| r.status == 200).expect("200");
    let schema = r.schema.as_ref().expect("schema");
    let resolved = schema.resolved.as_ref().expect("resolved");
    assert!(resolved.properties.contains_key("refToA"));
    let ref_to_a = resolved.properties.get("refToA").unwrap();
    let inner = ref_to_a.resolved.as_ref().expect("inner resolved");
    assert!(inner.recursive, "refToA should be marked recursive");
    // recursive 节点的 properties 是空的（不再展开）
    assert!(inner.properties.is_empty());
}
