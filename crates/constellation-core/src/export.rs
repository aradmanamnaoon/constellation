//! Deterministic, local data export. Provider credentials and private runtime
//! state are intentionally outside this boundary.

use crate::{CoreError, MessageSender, Result, RoomId, StudioSnapshot};

/// Export only one room's messages to readable, attributed Markdown.
///
/// This function is pure and makes no network, filesystem or model calls. Its
/// caller must obtain explicit user permission before writing or sharing the
/// result. Content is quoted as text so a model's HTML/Markdown directives do
/// not unexpectedly become active markup in a rendered export.
pub fn export_room_markdown(snapshot: &StudioSnapshot, room_id: RoomId) -> Result<String> {
    let room = snapshot
        .rooms
        .iter()
        .find(|room| room.id == room_id)
        .ok_or(CoreError::NotFound("room"))?;

    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&escape_markdown(&room.title));
    out.push_str("\n\n");
    out.push_str("Room ID: `");
    out.push_str(&room.id.to_string());
    out.push_str("`  \n");
    out.push_str("Format: Constellation local room transcript\n\n");
    out.push_str("---\n\n");

    for message in snapshot
        .messages
        .iter()
        .filter(|message| message.room_id == room_id)
    {
        let author = match message.sender {
            MessageSender::User => "You".to_owned(),
            MessageSender::System => "System".to_owned(),
            MessageSender::Agent(agent_id) => snapshot
                .agents
                .iter()
                .find(|agent| agent.id == agent_id)
                .map_or_else(
                    || format!("Unknown agent ({agent_id})"),
                    |agent| agent.name.clone(),
                ),
        };
        out.push_str("### ");
        out.push_str(&escape_markdown(&author));
        out.push_str("\n\n");
        out.push_str("Message ID: `");
        out.push_str(&message.id.to_string());
        out.push_str("`  \n");
        out.push_str("Time (Unix ms): `");
        out.push_str(&message.created_at_ms.to_string());
        out.push_str("`\n\n");
        if message.content.is_empty() {
            out.push_str("> *(empty message)*\n\n");
        } else {
            for line in message.content.lines() {
                out.push_str("> ");
                out.push_str(&escape_markdown(line));
                out.push('\n');
            }
            out.push('\n');
        }
    }
    Ok(out)
}

fn escape_markdown(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '*' | '_' | '[' | ']' | '(' | ')' | '`' | '<' | '>' | '&' | '#' | '!' | '|'
            | '~' => {
                // HTML entities keep the content visible but prevent inline
                // executable HTML and accidental Markdown formatting.
                let replacement = match character {
                    '<' => "&lt;",
                    '>' => "&gt;",
                    '&' => "&amp;",
                    _ => "",
                };
                if replacement.is_empty() {
                    result.push('\\');
                    result.push(character);
                } else {
                    result.push_str(replacement);
                }
            }
            _ => result.push(character),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::{Agent, AgentId, Message, MessageId, MessageSender, Room, RoomId, StudioSnapshot};

    use super::export_room_markdown;

    #[test]
    fn export_filters_room_and_does_not_export_provider_credentials() {
        let first = RoomId::new();
        let other = RoomId::new();
        let agent_id = AgentId::new();
        let snapshot = StudioSnapshot {
            rooms: vec![
                Room {
                    id: first,
                    title: "Research <Lab>".into(),
                    created_at_ms: 1,
                    member_ids: None,
                },
                Room {
                    id: other,
                    title: "Private".into(),
                    created_at_ms: 2,
                    member_ids: None,
                },
            ],
            agents: vec![Agent {
                id: agent_id,
                role: "Reviewer".into(),
                name: "Agent *One*".into(),
                provider_id: None,
                created_at_ms: 3,
            }],
            messages: vec![
                Message {
                    id: MessageId::new(),
                    room_id: first,
                    sender: MessageSender::User,
                    content: "Hello\n<script>alert(1)</script>".into(),
                    created_at_ms: 4,
                },
                Message {
                    id: MessageId::new(),
                    room_id: other,
                    sender: MessageSender::User,
                    content: "DO NOT EXPORT THIS ROOM".into(),
                    created_at_ms: 5,
                },
                Message {
                    id: MessageId::new(),
                    room_id: first,
                    sender: MessageSender::Agent(agent_id),
                    content: "**Claim** requires evidence".into(),
                    created_at_ms: 6,
                },
            ],
            ..StudioSnapshot::default()
        };
        let export = export_room_markdown(&snapshot, first).expect("export valid room");
        assert!(export.contains("Research &lt;Lab&gt;"));
        assert!(export.contains("Agent \\*One\\*"));
        assert!(export.contains("&lt;script&gt;alert\\(1\\)&lt;/script&gt;"));
        assert!(!export.contains("DO NOT EXPORT THIS ROOM"));
        assert!(!export.contains("<script>"));
        assert!(export.contains("Claim"));
    }

    #[test]
    fn nonexistent_room_is_rejected() {
        let snapshot = StudioSnapshot::default();
        assert!(export_room_markdown(&snapshot, RoomId::new()).is_err());
    }

    #[test]
    fn deterministic_empty_room_export() {
        let id = RoomId::new();
        let mut snapshot = StudioSnapshot::default();
        snapshot.rooms.push(Room {
            id,
            title: "New room".into(),
            created_at_ms: 0,
            member_ids: None,
        });
        let first = export_room_markdown(&snapshot, id).unwrap();
        let second = export_room_markdown(&snapshot, id).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("# New room"));
    }
}
