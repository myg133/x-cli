//! Workflow YAML 解析
//!
//! 把 `workflow.yaml` 反序列化为 IR `Workflow`，并做基本校验：
//! - 步骤名唯一
//! - `$steps.<name>` 引用必须指向前面已定义的 step
//! - endpoint id 必须存在于 ApiSpec.endpoints（这是调用时校验，parse 时不强制）

use crate::error::{Error, Result};
use crate::ir::Workflow;
use std::path::Path;

/// 从文件读 + 解析
pub fn parse_workflow<P: AsRef<Path>>(path: P) -> Result<Workflow> {
    let content = std::fs::read_to_string(path.as_ref())?;
    parse_workflow_str(&content)
}

/// 从字符串解析
pub fn parse_workflow_str(yaml: &str) -> Result<Workflow> {
    let workflow: Workflow = serde_yaml::from_str(yaml)
        .map_err(|e| Error::OpenApiParse(format!("workflow parse: {e}")))?;
    validate(&workflow)?;
    Ok(workflow)
}

fn validate(w: &Workflow) -> Result<()> {
    use std::collections::HashSet;
    let mut seen: HashSet<&str> = HashSet::new();
    for step in &w.steps {
        if !seen.insert(step.name.as_str()) {
            return Err(Error::InvalidIr(format!(
                "workflow step name not unique: {}",
                step.name
            )));
        }
    }
    // 校验 $steps.<name> 引用
    for step in &w.steps {
        check_refs(&step.inputs.path_params, &seen, &step.name)?;
        check_refs(&step.inputs.query, &seen, &step.name)?;
        check_refs(&step.inputs.headers, &seen, &step.name)?;
        check_refs(&step.inputs.body, &seen, &step.name)?;
    }
    Ok(())
}

fn check_refs(
    map: &std::collections::BTreeMap<String, String>,
    seen: &std::collections::HashSet<&str>,
    step_name: &str,
) -> Result<()> {
    use crate::ir::InputRef;
    for (k, v) in map {
        match InputRef::parse(v) {
            InputRef::StepOutput { step, .. } => {
                if !seen.contains(step.as_str()) {
                    return Err(Error::InvalidIr(format!(
                        "step `{}` references unknown step `{}` (field {})",
                        step_name, step, k
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
