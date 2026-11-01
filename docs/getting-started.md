# Getting started with Constellation (experimental Windows desktop app)

This guide describes the features currently present in Constellation's desktop **source code**. The newer Evidence search and encrypted backup/recovery settings controls were exercised in an **unsigned installed Windows QA build**; see the [dated Windows evidence record](qa/windows-native-smoke.md). The actual backup/restore operation has native Rust test coverage, but has not been exercised through the installed GUI. Cross-platform operation remains unverified. You do not need a Constellation account. Your rooms, messages, tasks, and settings are stored in a local SQLite workspace. Connecting a hosted model is optional and can incur provider charges.

## Make your first room

1. Open the installed desktop app. In **Rooms**, choose **Create your first room** and name it.
2. Select **Add a teammate**, or open **Library** and choose **Add to team** for a role. Roles tell a model how to respond; they do not give it permission to read your files, run commands, or install software.
3. Write a message in the room and choose **Save in this room**. This saves your message locally. It does not contact an AI provider.
4. Your saved rooms and messages remain in the active local workspace when you restart the app. The workspace database itself is **not encrypted by the new backup feature**; use the encrypted backup steps below to create a separate protected copy.

## Connect a model (optional)

1. Open **Settings → Add provider**. Choose the API format your model service actually provides: **OpenAI Chat Completions**, **OpenAI Responses**, **Anthropic Messages**, **Google Gemini**, **native Ollama**, or **custom OpenAI-compatible**.
2. Enter a display name, API base URL, exact model ID, and API key if needed. Remote endpoints require HTTPS; HTTP is allowed for loopback services such as a local Ollama server. You must start any local model server yourself.
3. Save the connection. Saving it does **not** send a model request. **Test text chat** is optional; it sends one request to the chosen provider and may incur charges. A successful text test does not prove that tools, streaming, or other modalities are supported.
4. In a room, select **Model provider**, then **Ask selected** or **Ask team** (up to eight selected agents). Alternatively, assign a provider to each agent in **Settings → Models for individual agents** and choose **Use each agent's saved model**. Explicit requests can send room context to **each selected provider**. Hosted endpoints receive that context off-device.

API keys entered in the app are held only in memory for the current session; you may need to enter them again after restarting. Model replies are text. Selecting an agent role does not enable computer or filesystem tools. Provider support depends on the endpoint and model and has not been established universally.

## Find passages in pasted text with Evidence

**Evidence** is a local text-search tool for quickly locating quotes in a document you supply. It does not ask an AI model to write an answer.

1. Choose the **Evidence** icon in the left rail.
2. Paste the text or Markdown you want to inspect into **Document**. The limit is **128 KiB (131,072 UTF-8 bytes)**. For now there is no file picker, PDF reader, website search, or automatic search over your rooms and files.
3. Enter words in **Search terms** (up to **2,048 UTF-8 bytes**) and select **Find passages**.
4. The app can show up to **five** matching passages, each copied exactly from the pasted text. Each result shows a quote, its start/end UTF-8 byte offsets, and a SHA-256 fingerprint of the whole pasted document. Use **Copy quote and citation** if you want these details on the clipboard.
5. To begin another search, edit the document or search terms; earlier results are cleared. **Clear pasted text** removes the text, terms, and results from the current window.

Evidence uses keyword-based ranking. It may miss synonyms or ideas expressed in different words. A match is **not proof** that a claim is scientifically correct. The displayed name `pasted-document.md` is an internal label for the text you pasted, **not the original document title or a verified scholarly reference**. Keep the original document to check the quote and add a proper citation yourself. The search runs in memory on your computer; the Evidence action does not store its content, read local files, upload it, or call a model. Pasted text remains in the open interface until cleared or the app is closed.

## Create an encrypted workspace backup

**Settings → Advanced Studio → Encrypted local backup and recovery** provides an explicit, local way to protect a copy of your workspace.

1. Open **Settings**, then expand **Advanced Studio**.
2. Under **Create an encrypted workspace backup**, enter a **strong, unique passphrase**, then enter it again in **Confirm backup passphrase**. The two entries must match and contain **8–1,024 UTF-8 bytes** each; non-English characters may use more than one byte. Eight bytes is only a minimum, not a recommended password strength.
3. Select **Create encrypted backup**. The app takes a consistent snapshot of committed SQLite content (including data still in SQLite's write-ahead log), encrypts it, and creates a **new** `.cstlbkp` file. The app chooses a unique filename automatically, inside its own **application data → backups-and-recovery** directory. You cannot choose the backup output folder in this screen.
4. After success, use **Copy path** to capture the full local filename. **A path is not a copy of the file**: open that location in your file manager and copy the actual `.cstlbkp` file to another secure device or storage location.
5. Keep the passphrase separately in a safe place. The app **cannot reset or recover it**. The screen clears the create/confirm passphrase fields when the operation finishes, and also clears backup passphrase fields when you close Advanced Studio or leave Settings.

The encrypted package contains the active SQLite workspace's committed records, such as rooms, saved messages, agents, and tasks. **File attachments, other project files, and API keys are not included.** There is no scheduled automatic backup, cloud sync, or password escrow. The current backend supports at most **64 MiB of unencrypted SQLite image data** per package; if your database exceeds that limit or disk space is insufficient, creation fails. The local active workspace remains a separate, ordinary database.

## Verify and recover a backup to a new file

1. In **Settings → Advanced Studio**, find **Recover an encrypted backup**.
2. Enter or paste the **full local path** to an existing `.cstlbkp` file, including the filename. You may copy it from the success panel or from your file manager. The path must be at most **4,096 UTF-8 bytes**. There is no file-picker or upload step.
3. Enter the **original passphrase**, exactly as used to create the file (again **8–1,024 UTF-8 bytes**). Select **Verify and recover to a new file**.
4. On success, the app displays a **new `.sqlite3` recovery-file path** in the same application data **backups-and-recovery** directory. Use **Copy path** to locate it.

**Recovery does not open, merge, or replace the active workspace.** Your current rooms and messages remain as they were, and the recovered SQLite file is separate. It must be **inspected or migrated separately** before its content can be used in your active app. The recovery file is an **unencrypted SQLite database**: store it securely. The passphrase field is cleared after the attempt. An incorrect passphrase, damaged package, missing path, over-limit package, or insufficient disk space produces a failure without publishing a requested restored database. The source package is not changed.

Do not manually overwrite an open `constellation.sqlite3` or move recovered files into the application's live data directory while the app is running. Automatic adoption or merging of recovered workspaces is not yet part of this feature.

## Tasks and transcripts

- **Tasks → Plan task** saves instructions for a room and teammate. A task starts as **Pending · Not executed**. You can choose a model and select **Run text task** to request a **text-only model reply**, or **Cancel task**. These actions do not run coding tools or perform actions on your computer. Cancellation can discard a local result but cannot recall a request already sent to a remote provider.
- **Rooms → Room details → Copy transcript** puts Markdown on your clipboard. It does not save a file, contact an AI provider, or grant access to your filesystem.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Evidence or backup buttons disabled | These operations require the native desktop runtime; browser preview is view-only. Check required fields, byte limits, and whether a request is already running. |
| Evidence finds no passages | Try keywords present in the pasted text. This is lexical search, not full semantic understanding. |
| Backup creation fails | Passphrases must match and be 8–1,024 UTF-8 bytes. Check available disk space and the 64 MiB database limit. |
| Recovery fails | Check the **full path**, original passphrase, package integrity, available disk space, and whether the file was moved or renamed. |
| Recovery succeeded but old rooms still appear | Expected: recovery writes a **separate SQLite file** and does not switch the active workspace. |
| Model returns HTTP 401 or 404 | Check the API key/account permissions (401) or base URL and provider API format (404). |
| Local model cannot connect | Start the local model server and verify its configured loopback port and API format. |
| No macOS/Linux/Android installer | These platforms have not been established as released/tested by this guide. |

This is still an experimental app. See the [platform acceptance ledger](quality/acceptance-ledger.md), [earlier Windows smoke evidence](qa/windows-native-smoke.md), and [known incomplete features](roadmap.md). Prior mock-provider test results and `[FIXTURE ONLY]` text are not evidence of real model capabilities.
