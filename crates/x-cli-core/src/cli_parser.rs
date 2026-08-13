//! CLI 工具解析器
//!
//! 解析 FDE agent 按 CliSpec schema 写的 YAML 文件。
//! 格式见 `ir::CliSpec` / `ir::CliTool` / `ir::CliArg`。

use crate::ir::CliSpec;
use crate::{Error, Result};
use std::path::Path;

/// 从 YAML 文件路径解析 CliSpec。
pub fn parse_cli_spec(path: impl AsRef<Path>) -> Result<CliSpec> {
    let content = std::fs::read_to_string(path.as_ref())?;
    parse_cli_spec_str(&content)
}

/// 从 YAML 字符串解析 CliSpec。
pub fn parse_cli_spec_str(yaml: &str) -> Result<CliSpec> {
    let spec: CliSpec = serde_yaml::from_str(yaml)?;
    validate_cli_spec(&spec)?;
    Ok(spec)
}

/// 校验 CliSpec 合法性。
fn validate_cli_spec(spec: &CliSpec) -> Result<()> {
    for tool in &spec.tools {
        // name 不能为空
        if tool.name.is_empty() {
            return Err(Error::InvalidIr("CLI 工具 name 不能为空".into()));
        }
        // command 不能为空
        if tool.command.is_empty() {
            return Err(Error::InvalidIr(format!(
                "CLI 工具 `{}` 的 command 不能为空",
                tool.name
            )));
        }
        // 每个 arg：flag 和 position 不能同时存在
        for arg in &tool.args {
            if arg.flag.is_some() && arg.position.is_some() {
                return Err(Error::InvalidIr(format!(
                    "CLI 工具 `{}` 的参数 `{}` 不能同时有 flag 和 position",
                    tool.name, arg.name
                )));
            }
            // name 不能为空
            if arg.name.is_empty() {
                return Err(Error::InvalidIr(format!(
                    "CLI 工具 `{}` 有 nameless 参数",
                    tool.name
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::CliOutputType;

    #[test]
    fn parse_basic_cli_tools() {
        let yaml = r#"
tools:
  - name: kubectl_get_pods
    description: "列出指定命名空间的 Pod"
    command: kubectl
    subcommand: ["get", "pods"]
    args:
      - name: namespace
        flag: --namespace
        shorthand: "-n"
        required: true
        description: "命名空间"
        schema:
          name: string
          json_schema: {"type": "string"}
    output: json

  - name: docker_ps
    description: "列出运行中的容器"
    command: docker
    subcommand: ["ps"]
    args:
      - name: all
        flag: --all
        shorthand: "-a"
        description: "显示所有容器（含已停止的）"
        schema:
          name: boolean
          json_schema: {"type": "boolean"}
      - name: quiet
        flag: --quiet
        shorthand: "-q"
        description: "只显示容器 ID"
        schema:
          name: boolean
          json_schema: {"type": "boolean"}
    output: json
"#;
        let spec = parse_cli_spec_str(yaml).unwrap();
        assert_eq!(spec.tools.len(), 2);

        // kubectl_get_pods
        let kubectl = &spec.tools[0];
        assert_eq!(kubectl.name, "kubectl_get_pods");
        assert_eq!(kubectl.command, "kubectl");
        assert_eq!(kubectl.subcommand, vec!["get", "pods"]);
        assert_eq!(kubectl.args.len(), 1);
        assert_eq!(kubectl.args[0].name, "namespace");
        assert_eq!(kubectl.args[0].flag.as_deref(), Some("--namespace"));
        assert_eq!(kubectl.args[0].shorthand.as_deref(), Some("-n"));
        assert!(kubectl.args[0].required);
        assert_eq!(kubectl.output, CliOutputType::Json);

        // docker_ps
        let docker = &spec.tools[1];
        assert_eq!(docker.name, "docker_ps");
        assert_eq!(docker.args.len(), 2);
        assert!(!docker.args[0].required);
    }

    #[test]
    fn parse_positional_arg() {
        let yaml = r#"
tools:
  - name: kubectl_exec
    description: "在 Pod 内执行命令"
    command: kubectl
    subcommand: ["exec"]
    args:
      - name: pod_name
        position: 0
        required: true
        description: "Pod 名称"
        schema:
          name: string
          json_schema: {"type": "string"}
      - name: command
        position: 1
        required: true
        description: "要执行的命令"
        schema:
          name: string
          json_schema: {"type": "string"}
      - name: namespace
        flag: --namespace
        shorthand: "-n"
        description: "命名空间"
        schema:
          name: string
          json_schema: {"type": "string"}
    output: text
"#;
        let spec = parse_cli_spec_str(yaml).unwrap();
        assert_eq!(spec.tools.len(), 1);

        let tool = &spec.tools[0];
        let pos0 = &tool.args[0];
        assert_eq!(pos0.position, Some(0));
        assert!(pos0.required);
        assert!(pos0.flag.is_none());

        let pos1 = &tool.args[1];
        assert_eq!(pos1.position, Some(1));

        let flag_arg = &tool.args[2];
        assert!(flag_arg.position.is_none());
        assert_eq!(flag_arg.flag.as_deref(), Some("--namespace"));
    }

    #[test]
    fn validate_flag_and_position_mutex() {
        let yaml = r#"
tools:
  - name: bad_tool
    command: some_cmd
    args:
      - name: conflict
        flag: --name
        position: 0
        schema:
          name: string
          json_schema: {"type": "string"}
"#;
        let result = parse_cli_spec_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("flag") && err.contains("position"));
    }

    #[test]
    fn validate_empty_name() {
        let yaml = r#"
tools:
  - name: ""
    command: some_cmd
"#;
        let result = parse_cli_spec_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn validate_empty_command() {
        let yaml = r#"
tools:
  - name: my_tool
    command: ""
"#;
        let result = parse_cli_spec_str(yaml);
        assert!(result.is_err());
    }
}
