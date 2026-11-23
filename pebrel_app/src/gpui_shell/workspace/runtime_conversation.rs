//! Capture UI-owned identity, read native files off-thread, recheck before publishing.

use std::sync::Arc;

use super::{NebulaWorkspace, WorkspaceTab};
use crate::{
    gpui_shell::terminal::view::TerminalView,
    runtime_api::{ApiError, RuntimeCommand, RuntimeDispatch, conversation::Request},
};
use gpui::{App, Entity, WeakEntity};

impl NebulaWorkspace {
    pub(super) fn conversation_view(
        &self,
        window_id: u64,
        pane_id: u64,
        cx: &App,
    ) -> Result<Entity<TerminalView>, ApiError> {
        self.runtime_window_requested(Some(window_id))?;
        self.tabs
            .iter()
            .find_map(|tab| match tab {
                WorkspaceTab::Terminal { panes, .. } => {
                    panes.iter().find(|pane| pane.id == pane_id).map(|pane| pane.view.clone())
                },
                _ => None,
            })
            .filter(|view| view.read(cx).runtime_conversation_identity().is_ok())
            .ok_or_else(|| {
                ApiError::new("conversation_ended", "the Agent pane was closed or exited")
            })
    }
}

pub(super) fn dispatch_read(
    dispatch: &Arc<RuntimeDispatch>,
    workspace: &WeakEntity<NebulaWorkspace>,
    cx: &mut App,
) -> bool {
    let RuntimeCommand::Conversation {
        window_id,
        pane_id,
        request: Request::Read { identity, before, revision },
    } = &dispatch.command
    else {
        return false;
    };
    let prepared = workspace
        .upgrade()
        .ok_or_else(|| ApiError::new("target_not_found", "the workspace was closed"))
        .and_then(|workspace| workspace.read(cx).conversation_view(*window_id, *pane_id, cx))
        .and_then(|view| {
            view.read(cx)
                .runtime_conversation_source(identity)
                .map(|source| (view.downgrade(), source))
        });
    let (view, source) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            dispatch.respond(Err(error));
            return true;
        },
    };
    let current = source.identity.clone();
    let (before, revision, dispatch) = (*before, revision.clone(), dispatch.clone());
    let work = cx.background_executor().spawn(async move {
        crate::assistant_answer::conversation::read(source, before, revision.as_deref())
    });
    cx.spawn(async move |cx| {
        let result = work.await.and_then(|page| {
            view.update(cx, |view, _| {
                let mut result = view.runtime_conversation_status(&current)?;
                result
                    .as_object_mut()
                    .expect("conversation status")
                    .extend(page.as_object().expect("conversation page").clone());
                Ok(result)
            })
            .map_err(|_| ApiError::new("conversation_ended", "the Agent pane was closed"))?
        });
        dispatch.respond(result);
    })
    .detach();
    true
}
