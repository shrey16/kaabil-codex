use crate::codex::TurnContext;
use crate::config::Config;

pub(crate) fn build_agent_spawn_config(turn: &TurnContext) -> Result<Config, String> {
    let base_config = turn.client.config();
    let mut config = (*base_config).clone();
    config.model = Some(turn.client.get_model());
    config.model_provider = turn.client.get_provider();
    config.model_reasoning_effort = turn.client.get_reasoning_effort();
    config.model_reasoning_summary = turn.client.get_reasoning_summary();
    config.developer_instructions = turn.developer_instructions.clone();
    config.base_instructions = turn.base_instructions.clone();
    config.compact_prompt = turn.compact_prompt.clone();
    config.user_instructions = turn.user_instructions.clone();
    config.shell_environment_policy = turn.shell_environment_policy.clone();
    config.codex_linux_sandbox_exe = turn.codex_linux_sandbox_exe.clone();
    config.cwd = turn.cwd.clone();
    config
        .approval_policy
        .set(turn.approval_policy)
        .map_err(|err| format!("approval_policy is invalid: {err}"))?;
    config
        .sandbox_policy
        .set(turn.sandbox_policy.clone())
        .map_err(|err| format!("sandbox_policy is invalid: {err}"))?;
    Ok(config)
}
