//! Native-only origin fence for file reviews. A missing session binding never
//! becomes a manual approval, and no binding can be reconstructed from a
//! provider response, renderer IPC, or a checkpoint after process restart.
use constellation_core::{OperationId, StudioStore, TaskState};
use constellation_tools::OperationContext;

use crate::{native_agent_turn, tool_host, turn_checkpoint::TrustedTurnStore};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewOrigin {
    Manual(OperationContext),
    Model {
        context: OperationContext,
        revision: u64,
    },
}

impl ReviewOrigin {
    fn context(self) -> OperationContext {
        match self {
            Self::Manual(context) | Self::Model { context, .. } => context,
        }
    }
}

/// Called under the task/tool transition gate, before and after native consent.
/// A valid ToolHost pending ID must be joined to an independent native-only
/// origin registration AND the authoritative, still-pending task identity.
pub(crate) fn verify_review(
    core: &StudioStore,
    host: &tool_host::ToolHost,
    operation_id: OperationId,
    registered: Option<ReviewOrigin>,
    trusted: Option<&TrustedTurnStore>,
) -> Result<(), String> {
    let origin = registered.ok_or("Unregistered native file review was refused")?;
    let context = origin.context();
    if host.pending_task(operation_id) != Some(context.task_id) {
        return Err("Native file review operation no longer matches its origin".into());
    }
    let snapshot = core
        .snapshot()
        .map_err(|_| "Could not validate native review task")?;
    if !snapshot.tasks.iter().any(|task| {
        task.id == context.task_id
            && task.agent_id == context.agent_id
            && task.room_id == context.room_id
            && task.state == TaskState::Pending
    }) {
        return Err("Native file review task identity is no longer pending".into());
    }
    if let ReviewOrigin::Model { revision, .. } = origin {
        let trusted = trusted.ok_or("Authenticated model review checkpoint is unavailable")?;
        native_agent_turn::reconcile_review(trusted, core, context, revision)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_consent::NativeConsentProof;
    use constellation_core::{AgentId, OperationId, RoomId, TaskState};

    fn fixture() -> (
        tempfile::TempDir,
        StudioStore,
        tool_host::ToolHost,
        OperationContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let core = StudioStore::open(root.path().join("core.sqlite3")).unwrap();
        let room = core.create_room("Native review fixture").unwrap();
        let agent = core.add_agent("Writer", "No ambient access").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Create a reviewed note")
            .unwrap();
        let workspace = root.path().join("artifacts");
        let audit = root.path().join("audit");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&audit).unwrap();
        let host = tool_host::ToolHost::open(&workspace, &audit, [0x39; 32]).unwrap();
        (
            root,
            core,
            host,
            OperationContext {
                agent_id: agent.id,
                room_id: room.id,
                task_id: task.id,
            },
        )
    }

    #[test]
    fn lost_or_forged_origin_never_falls_through_as_manual_review() {
        let (root, core, mut host, context) = fixture();
        let prepared = host
            .prepare(
                context,
                "create_new",
                "secret.txt",
                Some("private fixture bytes"),
            )
            .unwrap();
        let id: OperationId = prepared["operation_id"].as_str().unwrap().parse().unwrap();
        let ticket = prepared["digest"].as_str().unwrap();

        // The persisted ToolHost operation alone is insufficient: e.g. a
        // vanished model-review registry must never be interpreted as manual.
        let failure = verify_review(&core, &host, id, None, None).unwrap_err();
        assert!(!failure.contains("private fixture bytes"));
        assert!(
            verify_review(
                &core,
                &host,
                id,
                Some(ReviewOrigin::Model {
                    context,
                    revision: 2
                }),
                None,
            )
            .is_err()
        );
        assert!(
            verify_review(
                &core,
                &host,
                id,
                Some(ReviewOrigin::Manual(OperationContext {
                    agent_id: AgentId::new(),
                    ..context
                })),
                None,
            )
            .is_err()
        );
        assert!(
            verify_review(
                &core,
                &host,
                OperationId::new(),
                Some(ReviewOrigin::Manual(context)),
                None,
            )
            .is_err()
        );
        assert!(
            verify_review(
                &core,
                &host,
                id,
                Some(ReviewOrigin::Manual(OperationContext {
                    room_id: RoomId::new(),
                    ..context
                })),
                None,
            )
            .is_err()
        );
        assert!(!root.path().join("artifacts/secret.txt").exists());

        // A legitimate manual action still uses the exact same OS-native
        // consent and ToolHost audit, with a valid native-only registration.
        verify_review(&core, &host, id, Some(ReviewOrigin::Manual(context)), None).unwrap();
        let reviewed = host.pending_native_approval(id, ticket).unwrap();
        let result = host
            .approve_and_execute(
                id,
                ticket,
                NativeConsentProof::fixture_for_test(&id.to_string(), &reviewed.digest),
            )
            .unwrap();
        assert_eq!(result["status"], "succeeded");
        assert!(
            verify_review(&core, &host, id, Some(ReviewOrigin::Manual(context)), None).is_err()
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("artifacts/secret.txt")).unwrap(),
            "private fixture bytes"
        );
    }

    #[test]
    fn cancellation_invalidates_registered_manual_review_before_native_consent() {
        let (root, core, mut host, context) = fixture();
        let prepared = host
            .prepare(context, "create_new", "cancelled.md", Some("never execute"))
            .unwrap();
        let id: OperationId = prepared["operation_id"].as_str().unwrap().parse().unwrap();
        verify_review(&core, &host, id, Some(ReviewOrigin::Manual(context)), None).unwrap();
        core.transition_task(
            context.task_id,
            TaskState::Pending,
            TaskState::Cancelled,
            OperationId::new(),
            None,
        )
        .unwrap();
        assert!(
            verify_review(&core, &host, id, Some(ReviewOrigin::Manual(context)), None).is_err()
        );
        assert!(!root.path().join("artifacts/cancelled.md").exists());
        host.deny(id).unwrap();
    }
}
