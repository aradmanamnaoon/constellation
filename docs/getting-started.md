# Getting started with Constellation (experimental Windows build)

This guide describes behavior **observed in the Windows development app**, not
the entire future product. No account is required. The application owns its
local state; connecting a hosted AI provider is optional and can incur charges.

## Start with a private room

1. Open the experimental Windows desktop app. From the initial screen, choose
   **Create your first room** and give it a name.
2. Choose **Add a teammate** or open **Library**. Select a role and **Add to
   team**. The catalogue contains 200 distinct chat-role instructions.
   Role selection does not grant tools, edit your files or install software.
3. Type into **Write a message to this room** and use **Save in this room**.
   This action saves your words locally, without contacting any AI provider.
4. Your rooms and messages survive application restart in a local SQLite
   database. This initial build does not yet include encrypted backups or
   automatic room deletion. Do not use it for sensitive data until that exists.

## Connect a model (optional)

1. Open **Settings → Add provider**.
2. Enter a display name, your provider's API **base URL**, its exact model ID,
   and API key if required. Remote URLs must use HTTPS; loopback HTTP supports
   local model services such as an already-running OpenAI-compatible server.
3. In the room, choose the provider in **Model provider**. The default is
   deliberately **Choose a provider…**; adding a provider does not authorize
   automatic network requests.
4. Select agents under **Agents · Change team**. Click **Ask selected** for
   one, or **Ask team** for up to eight. A request sends relevant room context
   to the **selected provider**. A local provider stays on the configured
   loopback endpoint; an HTTPS provider receives private content off-device.
5. If some agents fail, the app reports their individual status. A repeat
   request for the same user message skips agents whose answer was saved.
   Replies are ordinary model text, not evidence of tool execution.

**Keys are kept in memory only for this session**, not securely stored for
future restarts. You may need to enter the key again. The app does not yet
probe arbitrary provider capabilities, prices, supported modalities or
tool-calling formats. The target must support an OpenAI-style
`POST /chat/completions` route that returns `choices[0].message.content`.

## Tasks and transcripts

- **Tasks → Plan task** saves instructions, room and teammate as **Pending ·
  Not Executed**. The only supported task action is **Cancel**; there is no
  connected local tool runner in this build. A prompt claiming to have edited
  files does not make the task completed.
- **Rooms → Room details → Copy transcript** copies the selected room as
  Markdown to the clipboard. It does not write a file, send data to a model
  or grant filesystem access. Paste into a local editor if you want a copy.

## Troubleshooting

| Symptom | What it means / action |
| --- | --- |
| Ask controls disabled | Save a user message, add an agent, then explicitly select a provider; check no request is already in progress |
| Provider returns HTTP 401 | Re-enter a valid session API key and check the model/account permissions |
| Provider returns HTTP 404 | Check the base URL and whether `/chat/completions` exists at that prefix |
| Remote provider unreachable | Verify connectivity, proxy/firewall and HTTPS; there is no fallback to another service |
| Local endpoint unreachable | Start the model server yourself and confirm it listens on the exact localhost port |
| Task says Pending | This build can plan and cancel tasks, **not execute** them |
| No Android/macOS/Linux installer | Those platforms have not been built/tested or released |

The platform is experimental. See [verified QA evidence](qa/windows-native-smoke.md)
and [known incomplete features](roadmap.md). Do not treat deterministic
`[FIXTURE ONLY]` test messages as actual AI-generated answers.
