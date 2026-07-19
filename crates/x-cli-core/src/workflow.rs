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
    // 校验 depends_on 引用
    for step in &w.steps {
        for dep in &step.depends_on {
            if !seen.contains(dep.as_str()) {
                return Err(Error::InvalidIr(format!(
                    "step `{}` depends on unknown step `{}`",
                    step.name, dep
                )));
            }
            if dep == &step.name {
                return Err(Error::InvalidIr(format!(
                    "step `{}` depends on itself",
                    step.name
                )));
            }
        }
    }
    // 环检测
    detect_cycles(w)?;
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

/// 检测 workflow DAG 中的环
fn detect_cycles(w: &Workflow) -> Result<()> {
    use std::collections::HashMap;
    // 任何 step 有 depends_on 才走拓扑校验
    let has_depends = w.steps.iter().any(|s| !s.depends_on.is_empty());
    if !has_depends {
        return Ok(());
    }

    // 邻接表：step -> 它依赖的 step
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for step in &w.steps {
        deps.insert(
            step.name.as_str(),
            step.depends_on.iter().map(|s| s.as_str()).collect(),
        );
    }

    // 拓扑排序（Kahn's algorithm）检测环
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for (name, dep_list) in &deps {
        in_degree.insert(name, dep_list.len());
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| *n)
        .collect();
    // 稳定排序：按 step 在数组中的原始位置
    let order: Vec<&str> = w.steps.iter().map(|s| s.name.as_str()).collect();
    queue.sort_by_key(|n| order.iter().position(|x| x == n).unwrap_or(usize::MAX));

    let mut visited = 0;
    while let Some(name) = queue.pop() {
        visited += 1;
        // 找依赖了这个 step 的 step（反向遍历）
        for (n, dep_list) in &deps {
            if dep_list.contains(&name) {
                let d = in_degree.get_mut(n).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push(n);
                }
            }
        }
        queue.sort_by_key(|n| order.iter().position(|x| x == n).unwrap_or(usize::MAX));
    }

    if visited < deps.len() {
        return Err(Error::InvalidIr(
            "workflow has a cycle in depends_on".to_string(),
        ));
    }
    Ok(())
}
