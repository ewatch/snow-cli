//! Pure tree building for `flow show`.
//!
//! Flow Designer stores a flow's steps as rows in several tables (flow logic,
//! action instances, subflow instances), linked into a tree by `ui_id` and
//! `parent_ui_id` and ordered by an `order` string. These helpers turn such
//! rows into an ordered, path-numbered step list without any I/O.

use serde::Serialize;

/// Deepest nesting followed; Flow Designer flows are a handful of levels deep.
const MAX_DEPTH: usize = 32;

/// What a flow step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    /// Flow logic: If, Else, For Each, parallel blocks, End, ...
    Logic,
    Action,
    Subflow,
}

/// One flow component row, already reduced to the fields the tree needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub kind: StepKind,
    pub ui_id: String,
    pub parent_ui_id: String,
    /// Raw `order` value, e.g. `"4"` or `"11➛12"`.
    pub order: String,
    /// Logic definition, action type, or subflow name.
    pub type_name: String,
    /// Condition label (`If: Rejected`) or the step's comment.
    pub note: String,
}

/// One step of the printed tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Position in the tree, e.g. `1.2.3`.
    pub path: String,
    pub kind: StepKind,
    #[serde(rename = "type")]
    pub type_name: String,
    pub note: String,
}

/// Sort key of an `order` value: the number after the last `➛` (a step
/// moved in Flow Designer keeps `old➛new`), so `"11➛12"` sorts as 12.
/// Unparseable values sort last.
pub fn order_key(order: &str) -> u64 {
    order
        .rsplit('➛')
        .next()
        .and_then(|last| last.trim().parse::<u64>().ok())
        .unwrap_or(u64::MAX)
}

/// Steps in execution order with `1.2.3` paths. A component whose parent is
/// not among `components` is placed at the top level; components only
/// reachable through a parent cycle follow at the top level, so every row
/// appears exactly once.
pub fn build_steps(components: &[Component]) -> Vec<Step> {
    let known: std::collections::HashSet<&str> = components
        .iter()
        .map(|component| component.ui_id.as_str())
        .filter(|ui_id| !ui_id.is_empty())
        .collect();
    let is_root = |component: &Component| !known.contains(component.parent_ui_id.as_str());
    let mut visited = vec![false; components.len()];
    let mut steps = Vec::with_capacity(components.len());
    let roots = sorted_children(components, |component| is_root(component));
    let mut placed = walk(components, &roots, "", 0, 0, &mut visited, &mut steps);
    let rest = sorted_children(components, |_| true);
    while let Some(&index) = rest.iter().find(|&&index| !visited[index]) {
        placed = walk(
            components,
            &[index],
            "",
            placed,
            0,
            &mut visited,
            &mut steps,
        );
    }
    steps
}

fn sorted_children(components: &[Component], select: impl Fn(&Component) -> bool) -> Vec<usize> {
    let mut children: Vec<usize> = components
        .iter()
        .enumerate()
        .filter(|(_, component)| select(component))
        .map(|(index, _)| index)
        .collect();
    // Stable: equal orders keep row order.
    children.sort_by_key(|&index| order_key(&components[index].order));
    children
}

/// Append `children` (and their subtrees) numbered from `placed + 1`;
/// returns the last position used.
fn walk(
    components: &[Component],
    children: &[usize],
    prefix: &str,
    placed: usize,
    depth: usize,
    visited: &mut [bool],
    steps: &mut Vec<Step>,
) -> usize {
    let mut position = placed;
    for &index in children {
        if visited[index] {
            continue;
        }
        visited[index] = true;
        position += 1;
        let component = &components[index];
        let path = if prefix.is_empty() {
            position.to_string()
        } else {
            format!("{prefix}.{position}")
        };
        steps.push(Step {
            path: path.clone(),
            kind: component.kind,
            type_name: component.type_name.clone(),
            note: component.note.clone(),
        });
        if depth + 1 < MAX_DEPTH && !component.ui_id.is_empty() {
            let nested = sorted_children(components, |child| child.parent_ui_id == component.ui_id);
            walk(components, &nested, &path, 0, depth + 1, visited, steps);
        }
    }
    position
}

/// Top-level shape of a flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Structure {
    /// `parallel` when a top-level step is a parallel block, else `sequential`.
    pub mode: &'static str,
    /// Types of the top-level steps, in order.
    pub top_level: Vec<String>,
    /// Branches of the first top-level parallel block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches: Option<usize>,
}

/// Summarize the top level of `steps` (as built by [`build_steps`]).
pub fn structure(steps: &[Step]) -> Structure {
    let top: Vec<&Step> = steps
        .iter()
        .filter(|step| !step.path.contains('.'))
        .collect();
    let parallel = top
        .iter()
        .find(|step| step.kind == StepKind::Logic && is_parallel_block(&step.type_name));
    let branches = parallel.map(|block| {
        let prefix = format!("{}.", block.path);
        steps
            .iter()
            .filter(|step| {
                step.path
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.contains('.'))
            })
            .count()
    });
    Structure {
        mode: if parallel.is_some() {
            "parallel"
        } else {
            "sequential"
        },
        top_level: top.iter().map(|step| step.type_name.clone()).collect(),
        branches,
    }
}

fn is_parallel_block(type_name: &str) -> bool {
    let lower = type_name.to_ascii_lowercase();
    lower.contains("parallel") && !lower.contains("branch")
}

/// Distinct action types, sorted.
pub fn action_types(steps: &[Step]) -> Vec<String> {
    let mut types: Vec<String> = steps
        .iter()
        .filter(|step| step.kind == StepKind::Action)
        .map(|step| step.type_name.clone())
        .collect();
    types.sort();
    types.dedup();
    types
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(kind: StepKind, ui: &str, parent: &str, order: &str, name: &str) -> Component {
        Component {
            kind,
            ui_id: ui.to_string(),
            parent_ui_id: parent.to_string(),
            order: order.to_string(),
            type_name: name.to_string(),
            note: String::new(),
        }
    }

    /// The recorded shape of "Change - Normal - Authorize" (rows unordered).
    fn authorize() -> Vec<Component> {
        use StepKind::{Action, Logic};
        vec![
            component(Logic, "end2", "b2", "15", "End"),
            component(Action, "wait", "b2", "11➛12", "Wait For Condition"),
            component(Logic, "par", "", "1", "Do the following in Parallel"),
            component(Logic, "b1", "par", "2", "Parallel Branch"),
            component(
                Action,
                "check",
                "b1",
                "2➛3",
                "Check Change for User Approval",
            ),
            component(Action, "policy", "b1", "4", "Apply Change Approval Policy"),
            component(Logic, "if1", "b1", "5", "If"),
            component(Action, "upd", "if1", "6", "Update Record"),
            component(Action, "eval1", "b1", "7", "Evaluate Change Model"),
            component(Logic, "if2", "b1", "8", "If"),
            component(Action, "mail", "if2", "9", "Send Email"),
            component(Logic, "end1", "b1", "10", "End"),
            component(Logic, "b2", "par", "11", "Parallel Branch"),
            component(
                Action,
                "disregard",
                "b2",
                "13",
                "Disregard Change Request Approvals",
            ),
            component(Action, "eval2", "b2", "14", "Evaluate Change Model"),
        ]
    }

    #[test]
    fn order_key_uses_the_number_after_the_arrow() {
        assert_eq!(order_key("11➛12"), 12);
        assert_eq!(order_key("4"), 4);
        assert_eq!(order_key(""), u64::MAX);
        assert_eq!(order_key("x"), u64::MAX);
    }

    #[test]
    fn builds_the_authorize_tree_in_execution_order() {
        let steps = build_steps(&authorize());
        let rendered: Vec<String> = steps
            .iter()
            .map(|step| format!("{} {}", step.path, step.type_name))
            .collect();
        assert_eq!(
            rendered,
            [
                "1 Do the following in Parallel",
                "1.1 Parallel Branch",
                "1.1.1 Check Change for User Approval",
                "1.1.2 Apply Change Approval Policy",
                "1.1.3 If",
                "1.1.3.1 Update Record",
                "1.1.4 Evaluate Change Model",
                "1.1.5 If",
                "1.1.5.1 Send Email",
                "1.1.6 End",
                "1.2 Parallel Branch",
                "1.2.1 Wait For Condition",
                "1.2.2 Disregard Change Request Approvals",
                "1.2.3 Evaluate Change Model",
                "1.2.4 End",
            ]
        );
        let shape = structure(&steps);
        assert_eq!(shape.mode, "parallel");
        assert_eq!(shape.top_level, ["Do the following in Parallel"]);
        assert_eq!(shape.branches, Some(2));
        assert_eq!(
            action_types(&steps),
            [
                "Apply Change Approval Policy",
                "Check Change for User Approval",
                "Disregard Change Request Approvals",
                "Evaluate Change Model",
                "Send Email",
                "Update Record",
                "Wait For Condition",
            ]
        );
    }

    #[test]
    fn missing_parents_become_top_level_and_cycles_terminate() {
        use StepKind::{Action, Logic};
        let components = vec![
            component(Action, "a", "gone", "2", "Orphan"),
            component(Logic, "x", "y", "1", "X"),
            component(Logic, "y", "x", "1", "Y"),
            component(Action, "b", "", "1", "First"),
        ];
        let steps = build_steps(&components);
        let rendered: Vec<String> = steps
            .iter()
            .map(|step| format!("{} {}", step.path, step.type_name))
            .collect();
        // x and y only reference each other: neither is a root, so they
        // follow at the top level instead of looping or disappearing.
        assert_eq!(rendered, ["1 First", "2 Orphan", "3 X", "3.1 Y"]);
        let shape = structure(&steps);
        assert_eq!(shape.mode, "sequential");
        assert_eq!(shape.branches, None);
    }
}
