//! CLI 工具解析集成测试
//!
//! 从 fixture 加载真实 CliSpec YAML，验证解析结果。

use x_cli_core::{parse_cli_spec_str, CliOutputType, CliSpec};

const FIXTURE: &str = include_str!("fixtures/cli-tools-simple.yaml");

#[test]
fn parse_fixture_produces_correct_count() {
    let spec: CliSpec = parse_cli_spec_str(FIXTURE).unwrap();
    assert_eq!(spec.tools.len(), 4, "fixture 应包含 4 个 CLI 工具");
}

#[test]
fn kubectl_tools_have_flag_args() {
    let spec: CliSpec = parse_cli_spec_str(FIXTURE).unwrap();

    let get_pods = spec.tools.iter().find(|t| t.name == "kubectl_get_pods").unwrap();
    assert_eq!(get_pods.command, "kubectl");
    assert_eq!(get_pods.subcommand, &["get", "pods"]);
    assert!(get_pods.args[0].flag.is_some());
    assert!(get_pods.args[0].required);
    assert_eq!(get_pods.output, CliOutputType::Json);

    let get_deploy = spec.tools.iter().find(|t| t.name == "kubectl_get_deployments").unwrap();
    assert_eq!(get_deploy.args.len(), 2);
    // first arg: --namespace (required)
    assert!(get_deploy.args[0].required);
    // second arg: --all-namespaces (boolean flag, not required)
    assert!(!get_deploy.args[1].required);
}

#[test]
fn docker_tools_have_positional_and_flag_args() {
    let spec: CliSpec = parse_cli_spec_str(FIXTURE).unwrap();

    let logs = spec.tools.iter().find(|t| t.name == "docker_logs").unwrap();
    // position 0: container (required)
    assert_eq!(logs.args[0].position, Some(0));
    assert!(logs.args[0].required);
    assert!(logs.args[0].flag.is_none());
    // --tail
    assert_eq!(logs.args[1].flag.as_deref(), Some("--tail"));
    // -f / --follow
    assert_eq!(logs.args[2].shorthand.as_deref(), Some("-f"));
    assert_eq!(logs.output, CliOutputType::Text);
}

#[test]
fn all_tools_have_non_empty_names() {
    let spec: CliSpec = parse_cli_spec_str(FIXTURE).unwrap();
    for tool in &spec.tools {
        assert!(!tool.name.is_empty(), "tool name 不能为空");
        assert!(!tool.command.is_empty(), "tool command 不能为空");
    }
}

#[test]
fn no_arg_has_both_flag_and_position() {
    let spec: CliSpec = parse_cli_spec_str(FIXTURE).unwrap();
    for tool in &spec.tools {
        for arg in &tool.args {
            assert!(
                arg.flag.is_none() || arg.position.is_none(),
                "参数 `{}` 不能同时有 flag 和 position",
                arg.name
            );
        }
    }
}