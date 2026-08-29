//! Plan tasks: the prompt that asks the agent for a plan, and reading the plan
//! back out of its answer.

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use lgtm_protocol::Plan;

/// How much of the agent's answer an error quotes back.
const ERROR_TEXT_CHARS: usize = 2000;

const PROMPT_HEAD: &str = r#"You are planning work in this repository. Read the code you need to understand it; do not create or modify any files.

Goal:
"#;

const PROMPT_TAIL: &str = r#"

Break the goal into the smallest set of independent coding tasks that together achieve it. Each task must be self-contained: a coding agent with no other context will receive only its prompt and this repository.

Answer with a single ```json fenced block and nothing after it, in exactly this shape:
```json
{"steps": [{"key": "short-slug", "title": "One line", "prompt": "Full instructions for the coding agent.", "depends_on": ["other-key"]}]}
```
Rules: keys are unique lowercase slugs; depends_on lists keys of steps whose result this step builds on (empty when independent); order steps so dependencies come first; prefer 2–6 steps."#;

pub fn planning_prompt(goal: &str) -> String {
    format!("{PROMPT_HEAD}{goal}{PROMPT_TAIL}")
}

/// The follow-up on a plan task plans again rather than editing the old plan.
pub fn revision_prompt(goal: &str, revision: &str) -> String {
    format!("{}\n\nRevision request:\n{revision}", planning_prompt(goal))
}

pub fn extract_plan(text: &str) -> Result<Plan> {
    let json = last_json_block(text).unwrap_or_else(|| text.trim());
    let plan: Plan = serde_json::from_str(json)
        .map_err(|err| anyhow!("plan was not valid JSON: {err}\n{}", truncate(text)))?;
    check(&plan)?;
    Ok(plan)
}

/// The last ```json block, or the last fence holding something object-shaped.
pub(crate) fn last_json_block(text: &str) -> Option<&str> {
    let mut found = None;
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let tagged = after.starts_with("json");
        let body = after.strip_prefix("json").unwrap_or(after);
        let Some(end) = body.find("```") else { break };
        let block = &body[..end];
        if tagged || block.trim_start().starts_with('{') {
            found = Some(block);
        }
        rest = &body[end + 3..];
    }
    found
}

fn check(plan: &Plan) -> Result<()> {
    if plan.steps.is_empty() {
        return Err(anyhow!("plan is invalid: no steps"));
    }
    let mut keys = HashSet::new();
    for step in &plan.steps {
        if !keys.insert(step.key.as_str()) {
            return Err(anyhow!("plan is invalid: duplicate step key {}", step.key));
        }
    }
    for step in &plan.steps {
        for dep in &step.depends_on {
            if !keys.contains(dep.as_str()) {
                return Err(anyhow!(
                    "plan is invalid: step {} depends on unknown key {dep}",
                    step.key
                ));
            }
        }
    }
    Ok(())
}

fn truncate(text: &str) -> String {
    text.chars().take(ERROR_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_STEP: &str = r#"{"steps":[{"key":"a","title":"A","prompt":"do a"}]}"#;

    #[test]
    fn planning_prompt_carries_the_goal_and_the_shape() {
        let prompt = planning_prompt("add a /health endpoint");
        assert!(prompt.contains("add a /health endpoint"));
        assert!(prompt.contains(r#""steps""#));
        assert!(
            revision_prompt("goal", "smaller steps").contains("Revision request:\nsmaller steps")
        );
    }

    #[test]
    fn reads_a_fenced_block_after_prose() {
        let text = format!("Here is the plan.\n\n```json\n{ONE_STEP}\n```\n");
        let plan = extract_plan(&text).unwrap();
        assert_eq!(plan.steps[0].key, "a");
        assert!(plan.steps[0].depends_on.is_empty());
    }

    #[test]
    fn the_last_block_wins() {
        let text = format!(
            "```json\n{{\"steps\":[{{\"key\":\"old\",\"title\":\"O\",\"prompt\":\"p\"}}]}}\n```\nOn reflection:\n```json\n{ONE_STEP}\n```"
        );
        assert_eq!(extract_plan(&text).unwrap().steps[0].key, "a");
    }

    #[test]
    fn bare_json_needs_no_fence() {
        assert_eq!(
            extract_plan(&format!("\n  {ONE_STEP}  \n"))
                .unwrap()
                .steps
                .len(),
            1
        );
    }

    #[test]
    fn reports_unparsable_json() {
        let err = extract_plan("```json\nnot json\n```")
            .unwrap_err()
            .to_string();
        assert!(err.contains("plan was not valid JSON"), "{err}");
    }

    #[test]
    fn rejects_duplicate_keys_and_unknown_dependencies() {
        let dupes = r#"{"steps":[{"key":"a","title":"A","prompt":"p"},{"key":"a","title":"B","prompt":"p"}]}"#;
        let err = extract_plan(dupes).unwrap_err().to_string();
        assert!(err.contains("plan is invalid"), "{err}");

        let unknown = r#"{"steps":[{"key":"a","title":"A","prompt":"p","depends_on":["b"]}]}"#;
        let err = extract_plan(unknown).unwrap_err().to_string();
        assert!(err.contains("plan is invalid"), "{err}");

        let err = extract_plan(r#"{"steps":[]}"#).unwrap_err().to_string();
        assert!(err.contains("plan is invalid"), "{err}");
    }
}
