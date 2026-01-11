use crate::CodexThread;
use crate::agent::AgentStatus;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::thread_manager::SubagentInfo;
use crate::thread_manager::SubagentOutputSnapshot;
use crate::thread_manager::ThreadManagerState;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GroupChatSender;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use serde::Serialize;
use std::sync::Arc;
use std::sync::Weak;

/// Control-plane handle for multi-agent operations.
/// `AgentControl` is held by each session (via `SessionServices`). It provides capability to
/// spawn new agents and the inter-agent communication layer.
#[derive(Clone, Default)]
pub(crate) struct AgentControl {
    /// Weak handle back to the global thread registry/state.
    /// This is `Weak` to avoid reference cycles and shadow persistence of the form
    /// `ThreadManagerState -> CodexThread -> Session -> SessionServices -> ThreadManagerState`.
    manager: Weak<ThreadManagerState>,
}

impl AgentControl {
    /// Construct a new `AgentControl` that can spawn/message agents via the given manager state.
    pub(crate) fn new(manager: Weak<ThreadManagerState>) -> Self {
        Self { manager }
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    /// Spawn a new agent thread and submit the initial prompt.
    /// `parent_id` is recorded so the orchestrator can discover its subagents.
    ///
    /// If `headless` is true, a background drain task is spawned to prevent unbounded event growth
    /// of the channel queue when there is no client actively reading the thread events.
    pub(crate) async fn spawn_agent(
        &self,
        parent_id: ThreadId,
        config: crate::config::Config,
        prompt: String,
        headless: bool,
        persona: Option<String>,
    ) -> CodexResult<ThreadId> {
        let state = self.upgrade()?;
        let new_thread = state
            .spawn_new_thread_with_source(
                config,
                self.clone(),
                SessionSource::SubAgent(SubAgentSource::Other("collab".to_string())),
            )
            .await?;

        state
            .register_subagent(parent_id, new_thread.thread_id, persona)
            .await;

        if headless {
            spawn_headless_drain(
                Arc::clone(&new_thread.thread),
                Arc::clone(&state),
                new_thread.thread_id,
            );
        }

        self.send_prompt(new_thread.thread_id, prompt).await?;

        Ok(new_thread.thread_id)
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    /// Send a `user` prompt to an existing agent thread.
    pub(crate) async fn send_prompt(
        &self,
        agent_id: ThreadId,
        prompt: String,
    ) -> CodexResult<String> {
        let state = self.upgrade()?;
        state.reset_subagent_output(agent_id).await;
        state
            .send_op(
                agent_id,
                Op::UserInput {
                    items: vec![UserInput::Text { text: prompt }],
                    final_output_json_schema: None,
                },
            )
            .await
    }

    #[allow(dead_code)] // Used by multi-agent orchestration.
    pub(crate) async fn post_group_chat_message(
        &self,
        parent_id: ThreadId,
        text: String,
        sender: GroupChatSender,
    ) -> CodexResult<()> {
        let state = self.upgrade()?;
        state
            .send_op(
                parent_id,
                Op::GroupChatMessage {
                    text,
                    mentions: Vec::new(),
                    sender,
                },
            )
            .await?;
        Ok(())
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    /// Fetch the last known status for `agent_id`, returning `NotFound` when unavailable.
    pub(crate) async fn get_status(&self, agent_id: ThreadId) -> AgentStatus {
        let Ok(state) = self.upgrade() else {
            // No agent available if upgrade fails.
            return AgentStatus::NotFound;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return AgentStatus::NotFound;
        };
        thread.agent_status().await
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    pub(crate) async fn list_subagents(
        &self,
        parent_id: ThreadId,
    ) -> CodexResult<Vec<SubagentSummary>> {
        let state = self.upgrade()?;
        let mut subagents = state.subagents_for_parent(parent_id).await;
        subagents.sort_by(|(left, _), (right, _)| left.to_string().cmp(&right.to_string()));
        let mut out = Vec::with_capacity(subagents.len());
        for (id, SubagentInfo { persona, .. }) in subagents {
            let status = match state.get_thread(id).await {
                Ok(thread) => thread.agent_status().await,
                Err(_) => AgentStatus::NotFound,
            };
            out.push(SubagentSummary {
                id,
                status,
                persona,
            });
        }
        Ok(out)
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    pub(crate) async fn subagent_output(
        &self,
        parent_id: ThreadId,
        subagent_id: ThreadId,
        max_chars: Option<usize>,
    ) -> CodexResult<SubagentOutputSnapshot> {
        let state = self.upgrade()?;
        if !state.is_subagent_of(parent_id, subagent_id).await {
            return Err(CodexErr::ThreadNotFound(subagent_id));
        }
        state
            .subagent_output_snapshot(subagent_id, max_chars)
            .await
            .ok_or_else(|| CodexErr::ThreadNotFound(subagent_id))
    }

    #[allow(dead_code)] // Used by multi-agent orchestration.
    pub(crate) async fn subagent_persona(
        &self,
        subagent_id: ThreadId,
    ) -> CodexResult<Option<String>> {
        let state = self.upgrade()?;
        Ok(state
            .subagent_info(subagent_id)
            .await
            .and_then(|info| info.persona))
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    pub(crate) async fn is_subagent_of(
        &self,
        parent_id: ThreadId,
        subagent_id: ThreadId,
    ) -> CodexResult<bool> {
        let state = self.upgrade()?;
        Ok(state.is_subagent_of(parent_id, subagent_id).await)
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    pub(crate) async fn shutdown_agent(&self, agent_id: ThreadId) -> CodexResult<()> {
        let state = self.upgrade()?;
        state.send_op(agent_id, Op::Shutdown).await?;
        Ok(())
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    pub(crate) async fn forget_subagent(&self, agent_id: ThreadId) -> CodexResult<()> {
        let state = self.upgrade()?;
        state.remove_thread(agent_id).await;
        Ok(())
    }

    fn upgrade(&self) -> CodexResult<Arc<ThreadManagerState>> {
        self.manager
            .upgrade()
            .ok_or_else(|| CodexErr::UnsupportedOperation("thread manager dropped".to_string()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SubagentSummary {
    pub(crate) id: ThreadId,
    pub(crate) status: AgentStatus,
    pub(crate) persona: Option<String>,
}

/// When an agent is spawned "headless" (no UI/view attached), there may be no consumer polling
/// `CodexThread::next_event()`. The underlying event channel is unbounded, so the producer can
/// accumulate events indefinitely. This drain task prevents that memory growth by polling and
/// discarding events until shutdown.
fn spawn_headless_drain(
    thread: Arc<CodexThread>,
    state: Arc<ThreadManagerState>,
    agent_id: ThreadId,
) {
    tokio::spawn(async move {
        let mut saw_message_item_completed = false;
        loop {
            match thread.next_event().await {
                Ok(event) => match event.msg {
                    EventMsg::ItemCompleted(event) => {
                        if let Some(message) = subagent_message_from_item(&event.item) {
                            saw_message_item_completed = true;
                            record_and_post_subagent_message(&state, agent_id, message).await;
                        }
                    }
                    EventMsg::AgentMessage(event) => {
                        if !saw_message_item_completed
                            && let Some(message) = normalize_subagent_message(&event.message)
                        {
                            record_and_post_subagent_message(&state, agent_id, message).await;
                        }
                    }
                    EventMsg::AgentMessageDelta(event) => {
                        state
                            .record_subagent_delta(agent_id, event.delta.as_str())
                            .await;
                    }
                    EventMsg::AgentMessageContentDelta(event) => {
                        state
                            .record_subagent_delta(agent_id, event.delta.as_str())
                            .await;
                    }
                    EventMsg::AgentReasoning(event) => {
                        state
                            .record_subagent_reasoning_delta(agent_id, event.text.as_str())
                            .await;
                    }
                    EventMsg::AgentReasoningDelta(event) => {
                        state
                            .record_subagent_reasoning_delta(agent_id, event.delta.as_str())
                            .await;
                    }
                    EventMsg::AgentReasoningRawContent(event) => {
                        state
                            .record_subagent_reasoning_delta(agent_id, event.text.as_str())
                            .await;
                    }
                    EventMsg::AgentReasoningRawContentDelta(event) => {
                        state
                            .record_subagent_reasoning_delta(agent_id, event.delta.as_str())
                            .await;
                    }
                    EventMsg::ReasoningContentDelta(event) => {
                        state
                            .record_subagent_reasoning_delta(agent_id, event.delta.as_str())
                            .await;
                    }
                    EventMsg::ReasoningRawContentDelta(event) => {
                        state
                            .record_subagent_reasoning_delta(agent_id, event.delta.as_str())
                            .await;
                    }
                    EventMsg::ExecCommandBegin(event) => {
                        let command = event.command.join(" ");
                        state
                            .record_subagent_tool_event(agent_id, format!("exec begin: {command}"))
                            .await;
                    }
                    EventMsg::ExecCommandEnd(event) => {
                        let command = event.command.join(" ");
                        let exit_code = event.exit_code;
                        state
                            .record_subagent_tool_event(
                                agent_id,
                                format!("exec end: {command} (exit {exit_code})"),
                            )
                            .await;
                    }
                    EventMsg::McpToolCallBegin(event) => {
                        let server = event.invocation.server;
                        let tool = event.invocation.tool;
                        let call_id = event.call_id;
                        state
                            .record_subagent_tool_event(
                                agent_id,
                                format!("tool begin: {server}/{tool} ({call_id})"),
                            )
                            .await;
                    }
                    EventMsg::McpToolCallEnd(event) => {
                        let status = if event.is_success() { "ok" } else { "error" };
                        let server = event.invocation.server;
                        let tool = event.invocation.tool;
                        let call_id = event.call_id;
                        state
                            .record_subagent_tool_event(
                                agent_id,
                                format!("tool end: {server}/{tool} ({call_id}) {status}"),
                            )
                            .await;
                    }
                    EventMsg::WebSearchBegin(event) => {
                        let call_id = event.call_id;
                        state
                            .record_subagent_tool_event(
                                agent_id,
                                format!("web search begin: {call_id}"),
                            )
                            .await;
                    }
                    EventMsg::WebSearchEnd(event) => {
                        let call_id = event.call_id;
                        let query = event.query;
                        state
                            .record_subagent_tool_event(
                                agent_id,
                                format!("web search end: {call_id} ({query})"),
                            )
                            .await;
                    }
                    EventMsg::ShutdownComplete => {
                        state.remove_thread(agent_id).await;
                        break;
                    }
                    _ => {}
                },
                Err(err) => {
                    tracing::warn!("failed to receive event from agent: {err:?}");
                    break;
                }
            }
        }
    });
}

fn normalize_subagent_message(message: &str) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn subagent_message_from_item(item: &TurnItem) -> Option<String> {
    let TurnItem::AgentMessage(message) = item else {
        return None;
    };
    let mut text = String::new();
    for entry in &message.content {
        let AgentMessageContent::Text { text: chunk } = entry;
        text.push_str(chunk);
    }
    normalize_subagent_message(&text)
}

async fn record_and_post_subagent_message(
    state: &ThreadManagerState,
    agent_id: ThreadId,
    message: String,
) {
    state
        .record_subagent_message(agent_id, message.as_str())
        .await;
    if let Some(info) = state.subagent_info(agent_id).await {
        let sender = GroupChatSender::SubAgent {
            id: agent_id,
            persona: info.persona.clone(),
        };
        if let Err(err) = state
            .send_op(
                info.parent_id,
                Op::GroupChatMessage {
                    text: message,
                    mentions: Vec::new(),
                    sender,
                },
            )
            .await
        {
            tracing::warn!("failed to post subagent message to group chat: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_status_from_event;
    use codex_protocol::protocol::ErrorEvent;
    use codex_protocol::protocol::TurnAbortReason;
    use codex_protocol::protocol::TurnAbortedEvent;
    use codex_protocol::protocol::TurnCompleteEvent;
    use codex_protocol::protocol::TurnStartedEvent;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn send_prompt_errors_when_manager_dropped() {
        let control = AgentControl::default();
        let err = control
            .send_prompt(ThreadId::new(), "hello".to_string())
            .await
            .expect_err("send_prompt should fail without a manager");
        assert_eq!(
            err.to_string(),
            "unsupported operation: thread manager dropped"
        );
    }

    #[tokio::test]
    async fn get_status_returns_not_found_without_manager() {
        let control = AgentControl::default();
        let got = control.get_status(ThreadId::new()).await;
        assert_eq!(got, AgentStatus::NotFound);
    }

    #[tokio::test]
    async fn on_event_updates_status_from_task_started() {
        let status = agent_status_from_event(&EventMsg::TurnStarted(TurnStartedEvent {
            model_context_window: None,
        }));
        assert_eq!(status, Some(AgentStatus::Running));
    }

    #[tokio::test]
    async fn on_event_updates_status_from_task_complete() {
        let status = agent_status_from_event(&EventMsg::TurnComplete(TurnCompleteEvent {
            last_agent_message: Some("done".to_string()),
        }));
        let expected = AgentStatus::Completed(Some("done".to_string()));
        assert_eq!(status, Some(expected));
    }

    #[tokio::test]
    async fn on_event_updates_status_from_error() {
        let status = agent_status_from_event(&EventMsg::Error(ErrorEvent {
            message: "boom".to_string(),
            codex_error_info: None,
        }));

        let expected = AgentStatus::Errored("boom".to_string());
        assert_eq!(status, Some(expected));
    }

    #[tokio::test]
    async fn on_event_updates_status_from_turn_aborted() {
        let status = agent_status_from_event(&EventMsg::TurnAborted(TurnAbortedEvent {
            reason: TurnAbortReason::Interrupted,
        }));

        let expected = AgentStatus::Errored("Interrupted".to_string());
        assert_eq!(status, Some(expected));
    }

    #[tokio::test]
    async fn on_event_updates_status_from_shutdown_complete() {
        let status = agent_status_from_event(&EventMsg::ShutdownComplete);
        assert_eq!(status, Some(AgentStatus::Shutdown));
    }
}
