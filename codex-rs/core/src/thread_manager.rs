use crate::AuthManager;
#[cfg(any(test, feature = "test-support"))]
use crate::CodexAuth;
#[cfg(any(test, feature = "test-support"))]
use crate::ModelProviderInfo;
use crate::agent::AgentControl;
use crate::codex::Codex;
use crate::codex::CodexSpawnOk;
use crate::codex::INITIAL_SUBMIT_ID;
use crate::codex_thread::CodexThread;
use crate::config::Config;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::models_manager::manager::ModelsManager;
use crate::protocol::Event;
use crate::protocol::EventMsg;
use crate::protocol::SessionConfiguredEvent;
use crate::rollout::RolloutRecorder;
use crate::rollout::truncation;
use crate::skills::SkillsManager;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(any(test, feature = "test-support"))]
use tempfile::TempDir;
use tokio::sync::RwLock;

/// Represents a newly created Codex thread (formerly called a conversation), including the first event
/// (which is [`EventMsg::SessionConfigured`]).
pub struct NewThread {
    pub thread_id: ThreadId,
    pub thread: Arc<CodexThread>,
    pub session_configured: SessionConfiguredEvent,
}

#[derive(Debug, Clone)]
pub(crate) struct SubagentInfo {
    pub(crate) parent_id: ThreadId,
    pub(crate) persona: Option<String>,
    pub(crate) display_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SubagentOutput {
    partial: String,
    last_message: Option<String>,
    reasoning: String,
    tool_events: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SubagentOutputSnapshot {
    pub(crate) partial: Option<String>,
    pub(crate) last_message: Option<String>,
    pub(crate) reasoning: Option<String>,
    pub(crate) tool_events: Vec<String>,
}

const MAX_SUBAGENT_OUTPUT_CHARS: usize = 8000;
const MAX_SUBAGENT_REASONING_CHARS: usize = 8000;
const MAX_SUBAGENT_TOOL_EVENTS: usize = 200;

/// [`ThreadManager`] is responsible for creating threads and maintaining
/// them in memory.
pub struct ThreadManager {
    state: Arc<ThreadManagerState>,
    #[cfg(any(test, feature = "test-support"))]
    _test_codex_home_guard: Option<TempDir>,
}

/// Shared, `Arc`-owned state for [`ThreadManager`]. This `Arc` is required to have a single
/// `Arc` reference that can be downgraded to by `AgentControl` while preventing every single
/// function to require an `Arc<&Self>`.
pub(crate) struct ThreadManagerState {
    threads: Arc<RwLock<HashMap<ThreadId, Arc<CodexThread>>>>,
    subagents: Arc<RwLock<HashMap<ThreadId, SubagentInfo>>>,
    subagent_outputs: Arc<RwLock<HashMap<ThreadId, SubagentOutput>>>,
    auth_manager: Arc<AuthManager>,
    models_manager: Arc<ModelsManager>,
    skills_manager: Arc<SkillsManager>,
    session_source: SessionSource,
}

impl ThreadManager {
    pub fn new(
        codex_home: PathBuf,
        auth_manager: Arc<AuthManager>,
        session_source: SessionSource,
    ) -> Self {
        Self {
            state: Arc::new(ThreadManagerState {
                threads: Arc::new(RwLock::new(HashMap::new())),
                subagents: Arc::new(RwLock::new(HashMap::new())),
                subagent_outputs: Arc::new(RwLock::new(HashMap::new())),
                models_manager: Arc::new(ModelsManager::new(
                    codex_home.clone(),
                    auth_manager.clone(),
                )),
                skills_manager: Arc::new(SkillsManager::new(codex_home)),
                auth_manager,
                session_source,
            }),
            #[cfg(any(test, feature = "test-support"))]
            _test_codex_home_guard: None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Construct with a dummy AuthManager containing the provided CodexAuth.
    /// Used for integration tests: should not be used by ordinary business logic.
    pub fn with_models_provider(auth: CodexAuth, provider: ModelProviderInfo) -> Self {
        let temp_dir = tempfile::tempdir().unwrap_or_else(|err| panic!("temp codex home: {err}"));
        let codex_home = temp_dir.path().to_path_buf();
        let mut manager = Self::with_models_provider_and_home(auth, provider, codex_home);
        manager._test_codex_home_guard = Some(temp_dir);
        manager
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Construct with a dummy AuthManager containing the provided CodexAuth and codex home.
    /// Used for integration tests: should not be used by ordinary business logic.
    pub fn with_models_provider_and_home(
        auth: CodexAuth,
        provider: ModelProviderInfo,
        codex_home: PathBuf,
    ) -> Self {
        let auth_manager = AuthManager::from_auth_for_testing(auth);
        Self {
            state: Arc::new(ThreadManagerState {
                threads: Arc::new(RwLock::new(HashMap::new())),
                subagents: Arc::new(RwLock::new(HashMap::new())),
                subagent_outputs: Arc::new(RwLock::new(HashMap::new())),
                models_manager: Arc::new(ModelsManager::with_provider(
                    codex_home.clone(),
                    auth_manager.clone(),
                    provider,
                )),
                skills_manager: Arc::new(SkillsManager::new(codex_home)),
                auth_manager,
                session_source: SessionSource::Exec,
            }),
            _test_codex_home_guard: None,
        }
    }

    pub fn session_source(&self) -> SessionSource {
        self.state.session_source.clone()
    }

    pub fn skills_manager(&self) -> Arc<SkillsManager> {
        self.state.skills_manager.clone()
    }

    pub fn get_models_manager(&self) -> Arc<ModelsManager> {
        self.state.models_manager.clone()
    }

    pub async fn list_models(&self, config: &Config) -> Vec<ModelPreset> {
        self.state.models_manager.list_models(config).await
    }

    pub async fn list_thread_ids(&self) -> Vec<ThreadId> {
        self.state.threads.read().await.keys().copied().collect()
    }

    pub async fn list_subagent_ids(&self, parent_id: ThreadId) -> Vec<ThreadId> {
        let mut ids = self
            .state
            .subagents_for_parent(parent_id)
            .await
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        ids.sort_by_key(std::string::ToString::to_string);
        ids
    }

    pub async fn subagent_persona(&self, subagent_id: ThreadId) -> Option<String> {
        self.state
            .subagent_info(subagent_id)
            .await
            .and_then(|info| info.persona)
    }

    pub async fn subagent_display_name(&self, subagent_id: ThreadId) -> Option<String> {
        self.state
            .subagent_info(subagent_id)
            .await
            .and_then(|info| info.display_name)
    }

    pub async fn get_thread(&self, thread_id: ThreadId) -> CodexResult<Arc<CodexThread>> {
        self.state.get_thread(thread_id).await
    }

    pub async fn start_thread(&self, config: Config) -> CodexResult<NewThread> {
        self.state
            .spawn_thread(
                config,
                InitialHistory::New,
                Arc::clone(&self.state.auth_manager),
                self.agent_control(),
            )
            .await
    }

    /// Spawn a subagent attached to `parent_id` and send an initial prompt.
    pub async fn spawn_subagent(
        &self,
        parent_id: ThreadId,
        mut config: Config,
        prompt: String,
        persona: Option<String>,
        display_name: Option<String>,
    ) -> CodexResult<ThreadId> {
        config.developer_instructions = crate::agent_personas::with_subagent_instructions(
            config.developer_instructions.as_deref(),
            persona.as_deref(),
            parent_id,
        );
        self.agent_control()
            .spawn_agent(parent_id, config, prompt, true, persona, display_name)
            .await
    }

    pub async fn resume_thread_from_rollout(
        &self,
        config: Config,
        rollout_path: PathBuf,
        auth_manager: Arc<AuthManager>,
    ) -> CodexResult<NewThread> {
        let initial_history = RolloutRecorder::get_rollout_history(&rollout_path).await?;
        self.resume_thread_with_history(config, initial_history, auth_manager)
            .await
    }

    pub async fn resume_thread_with_history(
        &self,
        config: Config,
        initial_history: InitialHistory,
        auth_manager: Arc<AuthManager>,
    ) -> CodexResult<NewThread> {
        self.state
            .spawn_thread(config, initial_history, auth_manager, self.agent_control())
            .await
    }

    /// Removes the thread from the manager's internal map, though the thread is stored
    /// as `Arc<CodexThread>`, it is possible that other references to it exist elsewhere.
    /// Returns the thread if the thread was found and removed.
    pub async fn remove_thread(&self, thread_id: &ThreadId) -> Option<Arc<CodexThread>> {
        self.state.remove_thread(*thread_id).await
    }

    /// Fork an existing thread by taking messages up to the given position (not including
    /// the message at the given position) and starting a new thread with identical
    /// configuration (unless overridden by the caller's `config`). The new thread will have
    /// a fresh id. Pass `usize::MAX` to keep the full rollout history.
    pub async fn fork_thread(
        &self,
        nth_user_message: usize,
        config: Config,
        path: PathBuf,
    ) -> CodexResult<NewThread> {
        let history = RolloutRecorder::get_rollout_history(&path).await?;
        let history = truncate_before_nth_user_message(history, nth_user_message);
        self.state
            .spawn_thread(
                config,
                history,
                Arc::clone(&self.state.auth_manager),
                self.agent_control(),
            )
            .await
    }

    fn agent_control(&self) -> AgentControl {
        AgentControl::new(Arc::downgrade(&self.state))
    }
}

impl ThreadManagerState {
    pub(crate) async fn get_thread(&self, thread_id: ThreadId) -> CodexResult<Arc<CodexThread>> {
        let threads = self.threads.read().await;
        threads
            .get(&thread_id)
            .cloned()
            .ok_or_else(|| CodexErr::ThreadNotFound(thread_id))
    }

    pub(crate) async fn send_op(&self, thread_id: ThreadId, op: Op) -> CodexResult<String> {
        self.get_thread(thread_id).await?.submit(op).await
    }

    pub(crate) async fn remove_thread(&self, thread_id: ThreadId) -> Option<Arc<CodexThread>> {
        self.unregister_subagent(thread_id).await;
        self.threads.write().await.remove(&thread_id)
    }

    #[allow(dead_code)] // Used by upcoming multi-agent tooling.
    pub(crate) async fn spawn_new_thread(
        &self,
        config: Config,
        agent_control: AgentControl,
    ) -> CodexResult<NewThread> {
        self.spawn_new_thread_with_source(config, agent_control, self.session_source.clone())
            .await
    }

    pub(crate) async fn spawn_new_thread_with_source(
        &self,
        config: Config,
        agent_control: AgentControl,
        session_source: SessionSource,
    ) -> CodexResult<NewThread> {
        self.spawn_thread_with_source(
            config,
            InitialHistory::New,
            Arc::clone(&self.auth_manager),
            agent_control,
            session_source,
        )
        .await
    }

    pub(crate) async fn spawn_thread(
        &self,
        config: Config,
        initial_history: InitialHistory,
        auth_manager: Arc<AuthManager>,
        agent_control: AgentControl,
    ) -> CodexResult<NewThread> {
        self.spawn_thread_with_source(
            config,
            initial_history,
            auth_manager,
            agent_control,
            self.session_source.clone(),
        )
        .await
    }

    pub(crate) async fn spawn_thread_with_source(
        &self,
        config: Config,
        initial_history: InitialHistory,
        auth_manager: Arc<AuthManager>,
        agent_control: AgentControl,
        session_source: SessionSource,
    ) -> CodexResult<NewThread> {
        let CodexSpawnOk {
            codex, thread_id, ..
        } = Codex::spawn(
            config,
            auth_manager,
            Arc::clone(&self.models_manager),
            Arc::clone(&self.skills_manager),
            initial_history,
            session_source,
            agent_control,
        )
        .await?;
        self.finalize_thread_spawn(codex, thread_id).await
    }

    async fn finalize_thread_spawn(
        &self,
        codex: Codex,
        thread_id: ThreadId,
    ) -> CodexResult<NewThread> {
        let event = codex.next_event().await?;
        let session_configured = match event {
            Event {
                id,
                msg: EventMsg::SessionConfigured(session_configured),
            } if id == INITIAL_SUBMIT_ID => session_configured,
            _ => {
                return Err(CodexErr::SessionConfiguredNotFirstEvent);
            }
        };

        let thread = Arc::new(CodexThread::new(
            codex,
            session_configured.rollout_path.clone(),
        ));
        self.threads.write().await.insert(thread_id, thread.clone());

        Ok(NewThread {
            thread_id,
            thread,
            session_configured,
        })
    }

    pub(crate) async fn register_subagent(
        &self,
        parent_id: ThreadId,
        subagent_id: ThreadId,
        persona: Option<String>,
        display_name: Option<String>,
    ) {
        self.subagents.write().await.insert(
            subagent_id,
            SubagentInfo {
                parent_id,
                persona,
                display_name,
            },
        );
        self.subagent_outputs
            .write()
            .await
            .entry(subagent_id)
            .or_insert_with(SubagentOutput::default);
    }

    pub(crate) async fn unregister_subagent(&self, subagent_id: ThreadId) {
        self.subagents.write().await.remove(&subagent_id);
        self.subagent_outputs.write().await.remove(&subagent_id);
    }

    pub(crate) async fn subagents_for_parent(
        &self,
        parent_id: ThreadId,
    ) -> Vec<(ThreadId, SubagentInfo)> {
        self.subagents
            .read()
            .await
            .iter()
            .filter_map(|(id, info)| {
                if info.parent_id == parent_id {
                    Some((*id, info.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub(crate) async fn subagent_info(&self, subagent_id: ThreadId) -> Option<SubagentInfo> {
        self.subagents.read().await.get(&subagent_id).cloned()
    }

    pub(crate) async fn is_subagent_of(&self, parent_id: ThreadId, subagent_id: ThreadId) -> bool {
        self.subagents
            .read()
            .await
            .get(&subagent_id)
            .is_some_and(|info| info.parent_id == parent_id)
    }

    pub(crate) async fn record_subagent_delta(&self, subagent_id: ThreadId, delta: &str) {
        if let Some(output) = self.subagent_outputs.write().await.get_mut(&subagent_id) {
            output.push_delta(delta);
        }
    }

    pub(crate) async fn record_subagent_message(&self, subagent_id: ThreadId, message: &str) {
        if let Some(output) = self.subagent_outputs.write().await.get_mut(&subagent_id) {
            output.set_message(message);
        }
    }

    pub(crate) async fn reset_subagent_output(&self, subagent_id: ThreadId) {
        if let Some(output) = self.subagent_outputs.write().await.get_mut(&subagent_id) {
            output.reset_for_prompt();
        }
    }

    pub(crate) async fn record_subagent_reasoning_delta(&self, subagent_id: ThreadId, delta: &str) {
        if let Some(output) = self.subagent_outputs.write().await.get_mut(&subagent_id) {
            output.push_reasoning_delta(delta);
        }
    }

    pub(crate) async fn record_subagent_tool_event(&self, subagent_id: ThreadId, event: String) {
        if let Some(output) = self.subagent_outputs.write().await.get_mut(&subagent_id) {
            output.push_tool_event(event);
        }
    }

    pub(crate) async fn subagent_output_snapshot(
        &self,
        subagent_id: ThreadId,
        max_chars: Option<usize>,
    ) -> Option<SubagentOutputSnapshot> {
        self.subagent_outputs
            .read()
            .await
            .get(&subagent_id)
            .map(|output| output.snapshot(max_chars))
    }
}

impl SubagentOutput {
    fn push_delta(&mut self, delta: &str) {
        self.partial.push_str(delta);
        trim_to_max_chars(&mut self.partial, MAX_SUBAGENT_OUTPUT_CHARS);
    }

    fn push_reasoning_delta(&mut self, delta: &str) {
        self.reasoning.push_str(delta);
        trim_to_max_chars(&mut self.reasoning, MAX_SUBAGENT_REASONING_CHARS);
    }

    fn push_tool_event(&mut self, event: String) {
        self.tool_events.push(event);
        if self.tool_events.len() > MAX_SUBAGENT_TOOL_EVENTS {
            let overflow = self
                .tool_events
                .len()
                .saturating_sub(MAX_SUBAGENT_TOOL_EVENTS);
            self.tool_events.drain(..overflow);
        }
    }

    fn set_message(&mut self, message: &str) {
        self.last_message = Some(message.to_string());
        self.partial.clear();
    }

    fn reset_for_prompt(&mut self) {
        self.partial.clear();
        self.reasoning.clear();
        self.tool_events.clear();
    }

    fn snapshot(&self, max_chars: Option<usize>) -> SubagentOutputSnapshot {
        let partial = max_chars
            .and_then(|limit| trim_snapshot(self.partial.as_str(), limit))
            .or_else(|| {
                if self.partial.is_empty() {
                    None
                } else {
                    Some(self.partial.clone())
                }
            });
        let reasoning = max_chars
            .and_then(|limit| trim_snapshot(self.reasoning.as_str(), limit))
            .or_else(|| {
                if self.reasoning.is_empty() {
                    None
                } else {
                    Some(self.reasoning.clone())
                }
            });
        SubagentOutputSnapshot {
            partial,
            last_message: self.last_message.clone(),
            reasoning,
            tool_events: self.tool_events.clone(),
        }
    }
}

fn trim_to_max_chars(value: &mut String, max_chars: usize) {
    let total = value.chars().count();
    if total <= max_chars {
        return;
    }
    let trim_chars = total.saturating_sub(max_chars);
    let start = value
        .char_indices()
        .nth(trim_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    value.drain(..start);
}

fn trim_snapshot(value: &str, max_chars: usize) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let total = value.chars().count();
    if total <= max_chars {
        return Some(value.to_string());
    }
    let trim_chars = total.saturating_sub(max_chars);
    let start = value
        .char_indices()
        .nth(trim_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    Some(value[start..].to_string())
}

/// Return a prefix of `items` obtained by cutting strictly before the nth user message
/// (0-based) and all items that follow it.
fn truncate_before_nth_user_message(history: InitialHistory, n: usize) -> InitialHistory {
    let items: Vec<RolloutItem> = history.get_rollout_items();
    let rolled = truncation::truncate_rollout_before_nth_user_message_from_start(&items, n);

    if rolled.is_empty() {
        InitialHistory::New
    } else {
        InitialHistory::Forked(rolled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::make_session_and_context;
    use assert_matches::assert_matches;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ReasoningItemReasoningSummary;
    use codex_protocol::models::ResponseItem;
    use pretty_assertions::assert_eq;

    fn user_msg(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::OutputText {
                text: text.to_string(),
            }],
        }
    }
    fn assistant_msg(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: text.to_string(),
            }],
        }
    }

    #[test]
    fn drops_from_last_user_only() {
        let items = [
            user_msg("u1"),
            assistant_msg("a1"),
            assistant_msg("a2"),
            user_msg("u2"),
            assistant_msg("a3"),
            ResponseItem::Reasoning {
                id: "r1".to_string(),
                summary: vec![ReasoningItemReasoningSummary::SummaryText {
                    text: "s".to_string(),
                }],
                content: None,
                encrypted_content: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                call_id: "c1".to_string(),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
            },
            assistant_msg("a4"),
        ];

        let initial: Vec<RolloutItem> = items
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect();
        let truncated = truncate_before_nth_user_message(InitialHistory::Forked(initial), 1);
        let got_items = truncated.get_rollout_items();
        let expected_items = vec![
            RolloutItem::ResponseItem(items[0].clone()),
            RolloutItem::ResponseItem(items[1].clone()),
            RolloutItem::ResponseItem(items[2].clone()),
        ];
        assert_eq!(
            serde_json::to_value(&got_items).unwrap(),
            serde_json::to_value(&expected_items).unwrap()
        );

        let initial2: Vec<RolloutItem> = items
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect();
        let truncated2 = truncate_before_nth_user_message(InitialHistory::Forked(initial2), 2);
        assert_matches!(truncated2, InitialHistory::New);
    }

    #[tokio::test]
    async fn ignores_session_prefix_messages_when_truncating() {
        let (session, turn_context) = make_session_and_context().await;
        let mut items = session.build_initial_context(&turn_context);
        items.push(user_msg("feature request"));
        items.push(assistant_msg("ack"));
        items.push(user_msg("second question"));
        items.push(assistant_msg("answer"));

        let rollout_items: Vec<RolloutItem> = items
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect();

        let truncated = truncate_before_nth_user_message(InitialHistory::Forked(rollout_items), 1);
        let got_items = truncated.get_rollout_items();

        let expected: Vec<RolloutItem> = vec![
            RolloutItem::ResponseItem(items[0].clone()),
            RolloutItem::ResponseItem(items[1].clone()),
            RolloutItem::ResponseItem(items[2].clone()),
        ];

        assert_eq!(
            serde_json::to_value(&got_items).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
}
