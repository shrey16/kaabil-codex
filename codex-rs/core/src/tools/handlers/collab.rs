use crate::codex::TurnContext;
use crate::config::types::ToolPolicyToml;
use crate::error::CodexErr;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::GroupChatSender;
use codex_protocol::protocol::SessionSource;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::time::Duration;
use tokio::time::Instant;
use tokio::time::sleep;

pub struct CollabHandler;

pub(crate) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;
pub(crate) const MAX_WAIT_TIMEOUT_MS: i64 = 300_000;

#[derive(Debug, Deserialize)]
struct SpawnAgentArgs {
    message: String,
    persona: Option<String>,
    tool_allowlist: Option<Vec<String>>,
    tool_denylist: Option<Vec<String>>,
    shell_command_allowlist: Option<Vec<String>>,
    shell_command_denylist: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SendInputArgs {
    id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct WaitArgs {
    id: String,
    timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CloseAgentArgs {
    id: String,
    timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListAgentsArgs {}

#[derive(Debug, Deserialize)]
struct AgentOutputArgs {
    id: String,
    max_chars: Option<usize>,
}

#[derive(Debug, Serialize)]
struct AgentOutputResponse {
    id: ThreadId,
    status: AgentStatus,
    partial: Option<String>,
    last_message: Option<String>,
    reasoning: Option<String>,
    tool_events: Option<Vec<String>>,
}

#[async_trait]
impl ToolHandler for CollabHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            tool_name,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "collab handler received unsupported payload".to_string(),
                ));
            }
        };

        match tool_name.as_str() {
            "spawn_agent" => handle_spawn_agent(session, turn, arguments).await,
            "send_input" => handle_send_input(session, turn, arguments).await,
            "wait" => handle_wait(session, arguments).await,
            "close_agent" => handle_close_agent(session, arguments).await,
            "list_agents" => handle_list_agents(session, arguments).await,
            "agent_output" => handle_agent_output(session, arguments).await,
            other => Err(FunctionCallError::RespondToModel(format!(
                "unsupported collab tool {other}"
            ))),
        }
    }
}

async fn handle_spawn_agent(
    session: std::sync::Arc<crate::codex::Session>,
    turn: std::sync::Arc<TurnContext>,
    arguments: String,
) -> Result<ToolOutput, FunctionCallError> {
    let args: SpawnAgentArgs = parse_arguments(&arguments)?;
    if args.message.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "Empty message can't be send to an agent".to_string(),
        ));
    }
    let SpawnAgentArgs {
        message,
        persona,
        tool_allowlist,
        tool_denylist,
        shell_command_allowlist,
        shell_command_denylist,
    } = args;
    let mut config = crate::agent::build_agent_spawn_config(turn.as_ref())
        .map_err(FunctionCallError::RespondToModel)?;
    let orchestrator_id = session.conversation_id();
    config.developer_instructions = crate::agent_personas::with_subagent_instructions(
        config.developer_instructions.as_deref(),
        persona.as_deref(),
        orchestrator_id,
    );
    config.tool_policy.apply_overrides(ToolPolicyToml {
        tool_allowlist,
        tool_denylist,
        shell_command_allowlist,
        shell_command_denylist,
    });
    let result = session
        .services
        .agent_control
        .spawn_agent(orchestrator_id, config, message, true, persona)
        .await
        .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;

    Ok(ToolOutput::Function {
        content: format!("agent_id: {result}"),
        success: Some(true),
        content_items: None,
    })
}

async fn handle_send_input(
    session: std::sync::Arc<crate::codex::Session>,
    turn: std::sync::Arc<TurnContext>,
    arguments: String,
) -> Result<ToolOutput, FunctionCallError> {
    let args: SendInputArgs = parse_arguments(&arguments)?;
    let message = args.message;
    if message.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "Empty message can't be send to an agent".to_string(),
        ));
    }
    let target_id = agent_id(&args.id)?;
    if matches!(turn.client.get_session_source(), SessionSource::SubAgent(_)) {
        let subagent_id = session.conversation_id();
        let is_parent = session
            .services
            .agent_control
            .is_subagent_of(target_id, subagent_id)
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
        if !is_parent {
            return Err(FunctionCallError::RespondToModel(format!(
                "agent with id {target_id} not found"
            )));
        }
        let persona = session
            .services
            .agent_control
            .subagent_persona(subagent_id)
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
        session
            .services
            .agent_control
            .post_group_chat_message(
                target_id,
                message,
                GroupChatSender::SubAgent {
                    id: subagent_id,
                    persona,
                },
            )
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
    } else {
        let parent_id = session.conversation_id();
        let is_subagent = session
            .services
            .agent_control
            .is_subagent_of(parent_id, target_id)
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
        if !is_subagent {
            return Err(FunctionCallError::RespondToModel(format!(
                "agent with id {target_id} not found"
            )));
        }
        session
            .process_group_chat_message(
                turn.sub_id.clone(),
                message,
                vec![target_id],
                GroupChatSender::TeamLead,
            )
            .await;
    }

    Ok(ToolOutput::Function {
        content: "ok".to_string(),
        success: Some(true),
        content_items: None,
    })
}

async fn handle_wait(
    session: std::sync::Arc<crate::codex::Session>,
    arguments: String,
) -> Result<ToolOutput, FunctionCallError> {
    let args: WaitArgs = parse_arguments(&arguments)?;
    let agent_id = agent_id(&args.id)?;
    let timeout_ms = resolve_timeout_ms(args.timeout_ms)?;
    let status = wait_for_agent(session, agent_id, timeout_ms).await?;
    Ok(ToolOutput::Function {
        content: status_payload(&status),
        success: Some(true),
        content_items: None,
    })
}

fn agent_id(id: &str) -> Result<ThreadId, FunctionCallError> {
    ThreadId::from_string(id)
        .map_err(|e| FunctionCallError::RespondToModel(format!("invalid agent id {id}: {e:?}")))
}

async fn handle_close_agent(
    session: std::sync::Arc<crate::codex::Session>,
    arguments: String,
) -> Result<ToolOutput, FunctionCallError> {
    let args: CloseAgentArgs = parse_arguments(&arguments)?;
    let agent_id = agent_id(&args.id)?;
    let parent_id = session.conversation_id();
    let is_subagent = session
        .services
        .agent_control
        .is_subagent_of(parent_id, agent_id)
        .await
        .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
    if !is_subagent {
        return Err(FunctionCallError::RespondToModel(format!(
            "agent with id {agent_id} not found"
        )));
    }
    session
        .services
        .agent_control
        .shutdown_agent(agent_id)
        .await
        .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
    let timeout_ms = resolve_timeout_ms(args.timeout_ms)?;
    let status = wait_for_agent(Arc::clone(&session), agent_id, timeout_ms).await?;
    session
        .services
        .agent_control
        .forget_subagent(agent_id)
        .await
        .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
    Ok(ToolOutput::Function {
        content: status_payload(&status),
        success: Some(true),
        content_items: None,
    })
}

async fn handle_list_agents(
    session: std::sync::Arc<crate::codex::Session>,
    arguments: String,
) -> Result<ToolOutput, FunctionCallError> {
    let _args: ListAgentsArgs = parse_arguments(&arguments)?;
    let parent_id = session.conversation_id();
    let summaries = session
        .services
        .agent_control
        .list_subagents(parent_id)
        .await
        .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
    let content = serde_json::to_string(&summaries)
        .unwrap_or_else(|_| format!("failed to serialize agent list: {summaries:?}"));
    Ok(ToolOutput::Function {
        content,
        success: Some(true),
        content_items: None,
    })
}

async fn handle_agent_output(
    session: std::sync::Arc<crate::codex::Session>,
    arguments: String,
) -> Result<ToolOutput, FunctionCallError> {
    let args: AgentOutputArgs = parse_arguments(&arguments)?;
    let agent_id = agent_id(&args.id)?;
    if matches!(args.max_chars, Some(0)) {
        return Err(FunctionCallError::RespondToModel(
            "max_chars must be greater than zero".to_string(),
        ));
    }
    let parent_id = session.conversation_id();
    let output = session
        .services
        .agent_control
        .subagent_output(parent_id, agent_id, args.max_chars)
        .await
        .map_err(|err| match err {
            CodexErr::ThreadNotFound(id) => {
                FunctionCallError::RespondToModel(format!("agent with id {id} not found"))
            }
            err => FunctionCallError::Fatal(err.to_string()),
        })?;
    let status = session.services.agent_control.get_status(agent_id).await;
    let tool_events = if output.tool_events.is_empty() {
        None
    } else {
        Some(output.tool_events)
    };
    let content = AgentOutputResponse {
        id: agent_id,
        status,
        partial: output.partial,
        last_message: output.last_message,
        reasoning: output.reasoning,
        tool_events,
    };
    let content = serde_json::to_string(&content)
        .unwrap_or_else(|_| format!("failed to serialize agent output: {content:?}"));
    Ok(ToolOutput::Function {
        content,
        success: Some(true),
        content_items: None,
    })
}

async fn wait_for_agent(
    session: std::sync::Arc<crate::codex::Session>,
    agent_id: ThreadId,
    timeout_ms: u64,
) -> Result<AgentStatus, FunctionCallError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let status = session.services.agent_control.get_status(agent_id).await;
        if !matches!(status, AgentStatus::PendingInit | AgentStatus::Running) {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(FunctionCallError::RespondToModel(format!(
                "wait timed out; last status was {status:?}"
            )));
        }
        sleep(Duration::from_millis(200)).await;
    }
}

fn resolve_timeout_ms(timeout_ms: Option<i64>) -> Result<u64, FunctionCallError> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);
    if timeout_ms <= 0 {
        return Err(FunctionCallError::RespondToModel(
            "timeout_ms must be greater than zero".to_string(),
        ));
    }
    Ok(timeout_ms.min(MAX_WAIT_TIMEOUT_MS) as u64)
}

fn status_payload(status: &AgentStatus) -> String {
    serde_json::to_string(status).unwrap_or_else(|_| format!("{status:?}"))
}
