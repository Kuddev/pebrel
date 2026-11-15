//! Managed names survive only with the exact native conversation that owns them.
//! Runtime ids, generations, worktree ownership and pending tasks never persist.

use super::*;

const MAX_PENDING_ALIAS_RESTORES: usize = 128;

pub(super) struct GenerationNamespace(u64);

impl GenerationNamespace {
    fn fresh() -> Self {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).expect("runtime Agent identity requires OS randomness");
        // Stay exactly representable in JSON clients, with room for generations.
        Self((u64::from_ne_bytes(bytes) & ((1_u64 << 51) - 1)) + 1)
    }
}

impl Default for GenerationNamespace {
    fn default() -> Self {
        #[cfg(test)]
        return Self(0);
        #[cfg(not(test))]
        Self::fresh()
    }
}

#[derive(Clone)]
pub(super) struct PendingAliasRestore {
    name: String,
    kind: crate::ai_agents::AgentKind,
    session_id: String,
}

impl RuntimeHub {
    /// A reset may first open a menu or leave the native id unavailable until
    /// the first new turn. Retire old authority as soon as input is accepted.
    pub(crate) fn retire_agents_for_local_reset(
        &self,
        window_id: u64,
        pane_id: u64,
        program: &str,
        text: &str,
    ) -> bool {
        if !crate::ai_hook::local_command::starts_new_conversation(program, text) {
            return false;
        }
        let mut state = self.lock();
        state.pending_alias_restores.remove(&(window_id, pane_id));
        for agent in state.managed_agents.values_mut().filter(|agent| {
            agent.active
                && agent.window_id == window_id
                && agent.pane_id == pane_id
                && agent.kind == program
        }) {
            agent.active = false;
            agent.closed_reason = Some("agent_replaced".into());
        }
        true
    }

    #[cfg(test)]
    pub(super) fn with_generation_namespace_for_test(namespace: u64) -> Self {
        let hub = Self::new();
        hub.lock().generation_namespace = GenerationNamespace(namespace);
        hub
    }

    /// Called on the UI thread after reading the current foreground identity.
    pub(crate) fn attach_agent(
        &self,
        name: String,
        kind: crate::ai_agents::AgentKind,
        window_id: u64,
        pane_id: u64,
        expected_session_id: &str,
        actual: &RuntimeAgent,
    ) -> Result<RuntimeManagedAgent, ApiError> {
        validate_agent_name(&name)?;
        if actual.kind != kind.slug() || actual.session_id.as_deref() != Some(expected_session_id) {
            return Err(ApiError::new(
                "agent_identity_mismatch",
                "the native Agent session changed",
            ));
        }
        let mut state = self.lock();
        for agent in state.managed_agents.values().filter(|agent| agent.active) {
            let same_pane = agent.window_id == window_id && agent.pane_id == pane_id;
            let same_native = agent.kind == actual.kind && agent.session_id == actual.session_id;
            if agent.name == name {
                if same_pane && same_native {
                    return Ok(agent.clone());
                }
                if !same_pane {
                    return Err(ApiError::new(
                        "agent_name_conflict",
                        "the name belongs to another pane",
                    ));
                }
            }
            if same_pane && same_native {
                return Err(ApiError::new(
                    "agent_pane_conflict",
                    "the native Agent already has another name",
                ));
            }
        }
        for agent in state.managed_agents.values_mut().filter(|agent| {
            agent.active && agent.window_id == window_id && agent.pane_id == pane_id
        }) {
            agent.active = false;
            agent.closed_reason = Some("agent_replaced".into());
        }
        let agent = register_agent_locked(
            &mut state,
            name,
            kind,
            window_id,
            pane_id,
            Some(expected_session_id.into()),
            None,
        )?;
        state.pending_alias_restores.remove(&(window_id, pane_id));
        Ok(agent)
    }

    pub(crate) fn register_agent(
        &self,
        name: String,
        kind: crate::ai_agents::AgentKind,
        window_id: u64,
        pane_id: u64,
        session_id: Option<String>,
        worktree: Option<crate::git_worktree::WorktreeProvenance>,
    ) -> Result<RuntimeManagedAgent, ApiError> {
        register_agent_locked(
            &mut self.lock(),
            name,
            kind,
            window_id,
            pane_id,
            session_id,
            worktree,
        )
    }

    /// Reserve metadata on the restored layout leaf, before the CLI is running.
    /// A saved recovery target is never itself evidence of a live conversation.
    pub(crate) fn restore_managed_alias(
        &self,
        window_id: u64,
        pane_id: u64,
        saved: &crate::session::AgentSession,
    ) {
        let Some(name) = saved.managed_name.as_deref() else { return };
        let Some(kind) = crate::ai_agents::AgentKind::parse(&saved.source) else { return };
        let Some(session_id) = saved.session_id.as_deref().filter(|id| !id.is_empty()) else {
            return;
        };
        if validate_agent_name(name).is_err() || saved.resume_command().is_none() {
            return;
        }
        let mut state = self.lock();
        if state.pending_alias_restores.len() >= MAX_PENDING_ALIAS_RESTORES {
            warn!("runtime managed alias restore limit reached");
            return;
        }
        state.pending_alias_restores.insert(
            (window_id, pane_id),
            PendingAliasRestore { name: name.into(), kind, session_id: session_id.into() },
        );
    }

    pub(crate) fn saved_managed_name(
        &self,
        window_id: u64,
        pane_id: u64,
        session: &crate::session::AgentSession,
    ) -> Option<String> {
        let session_id = session.session_id.as_deref()?;
        let state = self.lock();
        if let Some(agent) = state.managed_agents.values().find(|agent| {
            agent.active
                && agent.window_id == window_id
                && agent.pane_id == pane_id
                && agent.kind == session.source
                && agent.session_id.as_deref() == Some(session_id)
        }) {
            return Some(agent.name.clone());
        }
        state
            .pending_alias_restores
            .get(&(window_id, pane_id))
            .filter(|pending| {
                pending.kind.slug() == session.source && pending.session_id == session_id
            })
            .map(|pending| pending.name.clone())
    }
}

fn register_agent_locked(
    state: &mut HubState,
    name: String,
    kind: crate::ai_agents::AgentKind,
    window_id: u64,
    pane_id: u64,
    session_id: Option<String>,
    worktree: Option<crate::git_worktree::WorktreeProvenance>,
) -> Result<RuntimeManagedAgent, ApiError> {
    validate_agent_name(&name)?;
    if state.managed_agents.values().any(|agent| agent.active && agent.name == name) {
        return Err(ApiError::new(
            "agent_name_conflict",
            format!("an active agent named {name:?} already exists"),
        ));
    }
    if state
        .managed_agents
        .values()
        .any(|agent| agent.active && agent.window_id == window_id && agent.pane_id == pane_id)
    {
        return Err(ApiError::new("agent_pane_conflict", "this pane already has a managed Agent"));
    }
    let generation = state
        .agent_generations
        .get(&name)
        .copied()
        .unwrap_or(state.generation_namespace.0)
        .checked_add(1)
        .ok_or_else(|| ApiError::new("agent_identity_exhausted", "Agent generation exhausted"))?;
    state.agent_generations.insert(name.clone(), generation);
    let sequence = NEXT_AGENT_ID.fetch_add(1, Ordering::Relaxed);
    let agent_id =
        format!("agent-{}-{}-{sequence}", std::process::id(), state.generation_namespace.0);
    let agent = RuntimeManagedAgent {
        agent_id: agent_id.clone(),
        generation,
        name,
        kind: kind.slug().to_owned(),
        window_id,
        pane_id,
        session_id,
        worktree,
        active: true,
        observed: false,
        closed_reason: None,
    };
    state.managed_agents.insert(agent_id, agent.clone());
    Ok(agent)
}

pub(super) fn reconcile_alias_restores(state: &mut HubState, snapshot: &RuntimeSnapshot) {
    let pending = std::mem::take(&mut state.pending_alias_restores);
    for ((window_id, pane_id), saved) in pending {
        if state
            .pane_lifecycles
            .iter()
            .any(|event| event.window_id == window_id && event.pane_id == pane_id)
        {
            continue;
        }
        let actual = snapshot
            .windows
            .iter()
            .find(|window| window.id == window_id)
            .and_then(|window| {
                window.tabs.iter().flat_map(|tab| &tab.panes).find(|pane| pane.id == pane_id)
            })
            .and_then(|pane| pane.agent.as_ref());
        let Some(actual) = actual.filter(|agent| agent.session_id.is_some()) else {
            // Startup can publish another window before this pane is registered.
            state.pending_alias_restores.insert((window_id, pane_id), saved);
            continue;
        };
        if actual.kind != saved.kind.slug()
            || actual.session_id.as_deref() != Some(saved.session_id.as_str())
        {
            warn!("runtime discarded managed alias for a different restored Agent pane={pane_id}");
            continue;
        }
        if let Err(error) = register_agent_locked(
            state,
            saved.name,
            saved.kind,
            window_id,
            pane_id,
            Some(saved.session_id),
            None,
        ) {
            // Never steal an active name from another restored or attached pane.
            warn!("runtime managed alias restore rejected pane={pane_id} code={}", error.code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_namespaces_are_nonzero_and_json_safe() {
        let first = GenerationNamespace::fresh().0;
        let second = GenerationNamespace::fresh().0;
        assert!(first > 0 && first < (1_u64 << 52));
        assert!(second > 0 && second < (1_u64 << 52));
        assert_ne!(first, second);
    }
}
