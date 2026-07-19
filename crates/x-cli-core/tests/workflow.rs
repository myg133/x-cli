//! workflow.yaml 解析的回归测试

use x_cli_core::{parse_workflow_str, InputRef, StepInputs, Workflow, WorkflowInput, WorkflowStep};

const SIMPLE: &str = r#"
name: 简单两步工作流
description: |
  第一步创建，第二步读。
inputs:
  - name: petId
    type: string
    description: 宠物 ID
steps:
  - name: create
    description: 创建宠物
    endpoint: pet__post__pets
    inputs:
      body:
        name: "fluffy"
        tag: "$input.petId"
  - name: read
    description: 读宠物
    endpoint: pet__get__pets_petId
    inputs:
      path_params:
        petId: "$steps.create.response.body.id"
"#;

#[test]
fn parse_basic_workflow() {
    let wf = parse_workflow_str(SIMPLE).expect("parse");
    assert_eq!(wf.name, "简单两步工作流");
    assert_eq!(wf.steps.len(), 2);
    assert_eq!(wf.steps[0].name, "create");
    assert_eq!(wf.steps[1].name, "read");
    assert_eq!(wf.inputs.len(), 1);
    assert_eq!(wf.inputs[0].name, "petId");
}

#[test]
fn step_inputs_resolve_correctly() {
    let wf = parse_workflow_str(SIMPLE).expect("parse");
    let create = &wf.steps[0];
    let create_body = &create.inputs.body;
    let pet_id_ref = InputRef::parse(create_body.get("tag").expect("tag"));
    assert!(matches!(pet_id_ref, InputRef::Input(ref n) if n == "petId"));

    let read = &wf.steps[1];
    let path = read.inputs.path_params.get("petId").expect("petId path");
    let parsed = InputRef::parse(path);
    match parsed {
        InputRef::StepOutput { step, path } => {
            assert_eq!(step, "create");
            assert_eq!(path, vec!["response", "body", "id"]);
        }
        _ => panic!("expected StepOutput, got {parsed:?}"),
    }
}

#[test]
fn static_value_passes_through() {
    let wf = parse_workflow_str(SIMPLE).expect("parse");
    let name_ref = InputRef::parse(
        wf.steps[0].inputs.body.get("name").expect("name"),
    );
    assert!(matches!(name_ref, InputRef::Static(ref s) if s == "fluffy"));
}

#[test]
fn duplicate_step_name_is_rejected() {
    let yaml = r#"
name: dup
steps:
  - name: a
    endpoint: x__get__x
  - name: a
    endpoint: y__get__y
"#;
    let err = parse_workflow_str(yaml).expect_err("should fail");
    assert!(err.to_string().contains("not unique"));
}

#[test]
fn unknown_step_reference_is_rejected() {
    let yaml = r#"
name: bad-ref
steps:
  - name: a
    endpoint: x__get__x
    inputs:
      path_params:
        foo: "$steps.nonexistent.response.body.id"
"#;
    let err = parse_workflow_str(yaml).expect_err("should fail");
    assert!(err.to_string().contains("unknown step"));
}

#[test]
fn workflow_serializes_back() {
    let wf = parse_workflow_str(SIMPLE).expect("parse");
    let json = serde_json::to_string(&wf).expect("serialize");
    let back: Workflow = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.steps.len(), 2);
}

#[test]
fn empty_inputs_workflow_parses() {
    let yaml = r#"
name: minimal
steps:
  - name: ping
    endpoint: any__get__anything
"#;
    let wf = parse_workflow_str(yaml).expect("parse");
    assert_eq!(wf.steps.len(), 1);
    assert!(wf.inputs.is_empty());
    let s: &StepInputs = &wf.steps[0].inputs;
    assert!(s.path_params.is_empty());
}
