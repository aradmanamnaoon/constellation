# Constellation Core

Rust library for the local-first desktop application. The library stores rooms,
agents, messages, tasks, provider configuration, and an append-only event log
in an on-device SQLite database. It does not run agent tools or send network
requests.

## Minimum UI integration

```rust
use constellation_core::{MessageSender, StudioStore};

fn main() -> constellation_core::Result<()> {
    let store = StudioStore::open("constellation.sqlite")?;
    let room = store.create_room("Research")?;
    let agent = store.add_agent("Researcher", "Research bot")?;
    store.append_message(room.id, MessageSender::Agent(agent.id), "Ready")?;
    let task = store.create_task(room.id, agent.id, "Review the paper")?;
    println!("Task {} is {:?}", task.id, task.state);
    let snapshot = store.snapshot()?;
    assert_eq!(snapshot.messages.len(), 1);
    Ok(())
}
```

Every operation above commits both the new state and its event in one
transaction. For commands that might be retried (e.g. after an interrupted
desktop IPC call), use the `*_with_operation` methods with stable entity IDs
and a stable `OperationId`. A retry with identical input returns the original
result; different input with the same operation ID is rejected.

Task transitions use `transition_task(task_id, expected_state, next_state,
operation_id, failure_reason)` to atomically compare and update state.
Cancellation is terminal, so late completion cannot revive a cancelled task.
Call `events()` to read the persisted audit stream.

Provider profiles contain a model, a URL and a reference to an environment
variable or OS keyring entry. The library validates HTTPS for remote endpoints
and optionally allows HTTP on loopback interfaces. Credential values are
resolved by the host application at execution time; the adapter returns a
request description without authorization headers. Unknown fields such as
`api_key` are rejected when deserializing provider profiles.

`PermissionGate::new()` grants no tool actions. Tools must present an exact
`PermissionScope` that has been explicitly granted in memory for that agent,
room, task, action and resource. Permissions are never implicitly restored on
restart. The eventual tool runner must canonicalize resource identifiers
before checking permissions and must call `authorize` immediately before any
side effect. The core crate itself has no execution surface.

## Verification

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```
