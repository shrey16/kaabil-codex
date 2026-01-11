use codex_protocol::ThreadId;
pub(crate) const ORCHESTRATOR_PROMPT: &str = include_str!("../orchestrator_prompt.md");
pub(crate) const SUBAGENT_PROMPT: &str = include_str!("../subagent_prompt.md");

#[derive(Debug, Clone)]
pub(crate) struct SubagentTemplate {
    pub(crate) persona: &'static str,
    pub(crate) initial_message: &'static str,
}

pub(crate) const DEFAULT_SUBAGENT_TEMPLATES: &[SubagentTemplate] = &[
    SubagentTemplate {
        persona: "Planner: break work into steps, flag dependencies, and propose parallel splits.",
        initial_message: "You are the Planner subagent. Reply \"Ready\" and wait for assignments.",
    },
    SubagentTemplate {
        persona: "Builder: focus on implementation details and concrete code changes.",
        initial_message: "You are the Builder subagent. Reply \"Ready\" and wait for assignments.",
    },
    SubagentTemplate {
        persona: "Reviewer: focus on correctness, edge cases, and tests.",
        initial_message: "You are the Reviewer subagent. Reply \"Ready\" and wait for assignments.",
    },
];

pub(crate) fn with_orchestrator_instructions(existing: Option<&str>) -> Option<String> {
    merge_instructions(existing, ORCHESTRATOR_PROMPT)
}

pub(crate) fn with_subagent_instructions(
    existing: Option<&str>,
    persona: Option<&str>,
    orchestrator_id: ThreadId,
) -> Option<String> {
    let mut addition = String::new();
    if let Some(persona) = persona.and_then(non_empty_trimmed) {
        addition.push_str("Persona:\n");
        addition.push_str(persona);
        addition.push_str("\n\n");
    }
    addition.push_str(SUBAGENT_PROMPT.trim());
    addition.push_str("\n\n");
    addition.push_str(&format!("Orchestrator thread id: {orchestrator_id}"));

    merge_instructions(existing, addition.as_str())
}

fn merge_instructions(existing: Option<&str>, addition: &str) -> Option<String> {
    let addition = addition.trim();
    if addition.is_empty() {
        return existing.map(str::to_string);
    }
    if let Some(existing) = existing.and_then(non_empty_trimmed) {
        return Some(format!("{existing}\n\n{addition}"));
    }
    Some(addition.to_string())
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn merge_instructions_appends_with_spacing() {
        let got = merge_instructions(Some("alpha"), "beta");
        assert_eq!(got, Some("alpha\n\nbeta".to_string()));
    }

    #[test]
    fn merge_instructions_ignores_empty_addition() {
        let got = merge_instructions(Some("alpha"), "   ");
        assert_eq!(got, Some("alpha".to_string()));
    }

    #[test]
    fn subagent_instructions_include_orchestrator_id() {
        let id = ThreadId::default();
        let got = with_subagent_instructions(None, Some("researcher"), id)
            .expect("expected subagent instructions");
        assert!(got.contains("Persona:\nresearcher"));
        assert!(got.contains(&format!("Orchestrator thread id: {id}")));
    }
}
