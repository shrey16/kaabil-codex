//! Session-wide mutable state.

use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::GroupChatMessageEvent;

use crate::codex::SessionConfiguration;
use crate::context_manager::ContextManager;
use crate::protocol::RateLimitSnapshot;
use crate::protocol::TokenUsage;
use crate::protocol::TokenUsageInfo;
use crate::truncate::TruncationPolicy;
use std::collections::HashMap;

const MAX_GROUP_CHAT_MESSAGES: usize = 500;

#[derive(Debug, Clone)]
pub(crate) struct GroupChatState {
    entries: Vec<GroupChatMessageEvent>,
    cursors: HashMap<ThreadId, usize>,
}

impl GroupChatState {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursors: HashMap::new(),
        }
    }

    pub(crate) fn append(&mut self, message: GroupChatMessageEvent) -> usize {
        self.entries.push(message);
        if self.entries.len() > MAX_GROUP_CHAT_MESSAGES {
            let overflow = self.entries.len().saturating_sub(MAX_GROUP_CHAT_MESSAGES);
            self.entries.drain(..overflow);
            for cursor in self.cursors.values_mut() {
                *cursor = cursor.saturating_sub(overflow);
            }
        }
        self.entries.len()
    }

    pub(crate) fn unread_messages(
        &self,
        subagent_id: ThreadId,
    ) -> (usize, Vec<GroupChatMessageEvent>) {
        let start = self.cursors.get(&subagent_id).copied().unwrap_or(0);
        let start = start.min(self.entries.len());
        let messages = self.entries[start..].to_vec();
        (self.entries.len(), messages)
    }

    pub(crate) fn mark_read(&mut self, subagent_id: ThreadId, cursor: usize) {
        self.cursors.insert(subagent_id, cursor);
    }
}

/// Persistent, session-scoped state previously stored directly on `Session`.
pub(crate) struct SessionState {
    pub(crate) session_configuration: SessionConfiguration,
    pub(crate) history: ContextManager,
    pub(crate) latest_rate_limits: Option<RateLimitSnapshot>,
    pub(crate) group_chat: GroupChatState,
}

impl SessionState {
    /// Create a new session state mirroring previous `State::default()` semantics.
    pub(crate) fn new(session_configuration: SessionConfiguration) -> Self {
        let history = ContextManager::new();
        Self {
            session_configuration,
            history,
            latest_rate_limits: None,
            group_chat: GroupChatState::new(),
        }
    }

    // History helpers
    pub(crate) fn record_items<I>(&mut self, items: I, policy: TruncationPolicy)
    where
        I: IntoIterator,
        I::Item: std::ops::Deref<Target = ResponseItem>,
    {
        self.history.record_items(items, policy);
    }

    pub(crate) fn clone_history(&self) -> ContextManager {
        self.history.clone()
    }

    pub(crate) fn replace_history(&mut self, items: Vec<ResponseItem>) {
        self.history.replace(items);
    }

    pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
        self.history.set_token_info(info);
    }

    // Token/rate limit helpers
    pub(crate) fn update_token_info_from_usage(
        &mut self,
        usage: &TokenUsage,
        model_context_window: Option<i64>,
    ) {
        self.history.update_token_info(usage, model_context_window);
    }

    pub(crate) fn token_info(&self) -> Option<TokenUsageInfo> {
        self.history.token_info()
    }

    pub(crate) fn set_rate_limits(&mut self, snapshot: RateLimitSnapshot) {
        self.latest_rate_limits = Some(merge_rate_limit_fields(
            self.latest_rate_limits.as_ref(),
            snapshot,
        ));
    }

    pub(crate) fn token_info_and_rate_limits(
        &self,
    ) -> (Option<TokenUsageInfo>, Option<RateLimitSnapshot>) {
        (self.token_info(), self.latest_rate_limits.clone())
    }

    pub(crate) fn set_token_usage_full(&mut self, context_window: i64) {
        self.history.set_token_usage_full(context_window);
    }

    pub(crate) fn get_total_token_usage(&self) -> i64 {
        self.history.get_total_token_usage()
    }
}

// Sometimes new snapshots don't include credits or plan information.
fn merge_rate_limit_fields(
    previous: Option<&RateLimitSnapshot>,
    mut snapshot: RateLimitSnapshot,
) -> RateLimitSnapshot {
    if snapshot.credits.is_none() {
        snapshot.credits = previous.and_then(|prior| prior.credits.clone());
    }
    if snapshot.plan_type.is_none() {
        snapshot.plan_type = previous.and_then(|prior| prior.plan_type);
    }
    snapshot
}
