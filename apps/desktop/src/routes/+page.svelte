<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import {
    Activity, ArrowRight, Bot, Check, ChevronDown,
    ChevronRight, Command, Layers3,
    Menu, MessageCircle, MoreHorizontal, Plus, Search, Settings2,
    ShieldCheck, SlidersHorizontal, Sparkles, Square, UsersRound, X,
    Cpu, BookOpen,
  } from '@lucide/svelte';
  import roleCatalog from '../../../../catalog/roles.json';
  import './studio.css';

  type Screen = 'home' | 'rooms' | 'tasks' | 'library' | 'settings';
  type Room = { id: string; name: string; description: string; member_ids: string[] };
  type Agent = { id: string; name: string; role: string; model?: string };
  type ChatMessage = { id: string; room_id: string; sender_kind: 'user' | 'agent' | 'system'; sender_name: string; body: string; created_at: number | string; simulated?: boolean };
  type Provider = { id: string; name: string; endpoint: string; model: string; connected: boolean };
  type StudioTask = { id: string; room_id: string; agent_id: string; instructions: string; state: 'pending' | 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'; failure_reason: string | null; created_at: number; updated_at: number };
  type TeamReply = { agent_id: string; status: 'succeeded' | 'failed' | 'skipped'; message_id?: string; reason?: 'already_answered' | 'stale'; error?: string };
  type TeamReplyResponse = { room_id: string; prompt_message_id: string; provider_id: string; results: unknown };
  type ReplyFeedback = { providerName: string; replies: TeamReply[]; requestedIds: string[]; error?: string };
  type PendingSend = { content: string; messageId: string; operationId: string };
  type Snapshot = { rooms: Room[]; agents: Agent[]; messages: ChatMessage[]; providers: Provider[]; tasks: StudioTask[] };
  type Dialog = null | 'new-room' | 'new-agent' | 'new-task' | 'provider' | 'commands';

  const starterRoles = roleCatalog.roles.map((role) => ({ name: role.name, icon: '✳', group: role.category, blurb: role.summary }));

  let screen = $state<Screen>('rooms');
  let dialog = $state<Dialog>(null);
  let snapshot = $state<Snapshot>({ rooms: [], agents: [], messages: [], providers: [], tasks: [] });
  let roomId = $state('');
  let draft = $state('');
  let draftsByRoom = $state<Record<string, string>>({});
  let pendingSendsByRoom = $state<Record<string, PendingSend>>({});
  let selectedProviderId = $state('');
  let selectedAgentsByRoom = $state<Record<string, string[]>>({});
  let replyBusy = $state(false);
  let replyFeedbackByRoom = $state<Record<string, ReplyFeedback>>({});
  let newRoom = $state('');
  let newAgent = $state('');
  let selectedRole = $state(starterRoles[0]?.name ?? '');
  let search = $state('');
  let providerName = $state('Custom provider');
  let providerEndpoint = $state('http://127.0.0.1:11434/v1');
  let providerModel = $state('');
  let providerKey = $state('');
  let taskRoomId = $state('');
  let taskFilterRoomId = $state('');
  let taskAgentId = $state('');
  let taskInstructions = $state('');
  let runningTaskId = $state('');
  let pendingTaskCreate = $state<{ key: string; taskId: string; operationId: string } | null>(null);
  let working = $state(false);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let sidebarOpen = $state(false);
  let showDetails = $state(false);
  let advanced = $state(false);
  let isNative = $state(false);
  let runtimeChecked = $state(false);
  // Bound the number of mounted message nodes even in very large rooms.
  const messagePageSize = 100;
  let messagePage = $state(0);

  const room = $derived(snapshot.rooms.find((r) => r.id === roomId));
  const messages = $derived(snapshot.messages.filter((m) => m.room_id === roomId));
  const maxMessagePage = $derived(Math.max(0, Math.ceil(messages.length / messagePageSize) - 1));
  const visibleMessageEnd = $derived(Math.max(0, messages.length - Math.min(messagePage, maxMessagePage) * messagePageSize));
  const visibleMessages = $derived(messages.slice(Math.max(0, visibleMessageEnd - messagePageSize), visibleMessageEnd));
  const selectedAgentIds = $derived((selectedAgentsByRoom[roomId] ?? snapshot.agents.slice(0, 1).map((agent) => agent.id)).filter((id) => snapshot.agents.some((agent) => agent.id === id)));
  const selectedProvider = $derived(snapshot.providers.find((provider) => provider.id === selectedProviderId));
  // Retrying an incomplete team request is legitimate even when one agent
  // already replied. The Rust backend skips already-answered agent IDs for the
  // most recent user prompt; don't strand failed teammates behind this gate.
  const canAsk = $derived(isNative && !busy && !replyBusy && !!roomId && !!selectedProvider && selectedAgentIds.length > 0 && messages.some((message) => message.sender_kind === 'user'));
  const roomTasks = $derived(snapshot.tasks.filter((task) => !taskFilterRoomId || task.room_id === taskFilterRoomId).slice().reverse());
  const filteredRoles = $derived(starterRoles.filter((role) => [role.name,role.blurb,role.group].join(' ').toLowerCase().includes(search.toLowerCase())));

  function timeDisplay(value: number | string): string {
    // Rust serializes created_at_ms as a number; historical ISO strings also work.
    const date = new Date(typeof value === 'number' ? value : /^\d{12,}$/.test(value) ? Number(value) : value);
    return Number.isNaN(date.valueOf()) ? '' : new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(date);
  }

  async function refresh() {
    isNative = isTauri();
    runtimeChecked = true;
    if (!isNative) return;
    try {
      const next = await invoke<Snapshot>('studio_bootstrap');
      snapshot = { ...next, tasks: next.tasks ?? [] };
      if (!next.rooms.some((r) => r.id === roomId)) selectRoom(next.rooms[0]?.id ?? '');
      if (selectedProviderId && !next.providers.some((provider) => provider.id === selectedProviderId)) selectedProviderId = '';
      if (taskFilterRoomId && !next.rooms.some((r) => r.id === taskFilterRoomId)) taskFilterRoomId = '';
      error = '';
    } catch (cause) {
      error = `Could not open local workspace: ${String(cause)}`;
    }
  }
  onMount(() => { void refresh(); });

  async function createRoom() {
    if (!newRoom.trim() || !isNative) return;
    working = true; error = '';
    try {
      const created = await invoke<Room>('studio_create_room', { title: newRoom.trim() });
      newRoom = '';
      await refresh(); selectRoom(created.id); dialog = null; screen = 'rooms';
    } catch (cause) { error = String(cause); }
    finally { working = false; }
  }

  async function createAgent(roleName: string, name: string) {
    if (!isNative) return;
    working = true; error = '';
    try {
      await invoke('studio_add_agent', { role: roleName, name: name.trim() || roleName });
      await refresh(); dialog = null; screen = 'rooms';
      notice = `${name.trim() || roleName} added. Choose a provider before asking this agent to reply.`;
    } catch (cause) { error = String(cause); }
    finally { working = false; }
  }

  async function sendMessage() {
    if (!draft.trim() || !roomId || !isNative || busy || replyBusy) return;
    const content = draft.trim();
    const targetRoomId = roomId;
    // Stable IDs let an uncertain native IPC failure be retried without
    // inserting the same user message twice.
    const previous = pendingSendsByRoom[targetRoomId];
    const pending = previous?.content === content ? previous : {
      content, messageId: crypto.randomUUID(), operationId: crypto.randomUUID()
    };
    pendingSendsByRoom = { ...pendingSendsByRoom, [targetRoomId]: pending };
    let persisted = false;
    busy = true; error = '';
    try {
      await invoke('studio_send_message', {
        roomId: targetRoomId, content, messageId: pending.messageId, operationId: pending.operationId
      });
      persisted = true;
      delete pendingSendsByRoom[targetRoomId];
      // A room switch or further typing can occur while native IPC is pending.
      // Clear only the original submitted text, never another room's draft.
      if (roomId === targetRoomId && draft.trim() === content) draft = '';
      if ((draftsByRoom[targetRoomId] ?? '').trim() === content) {
        draftsByRoom = { ...draftsByRoom, [targetRoomId]: '' };
      }
      await refresh();
      // Sending saves locally. External inference is a separate, explicit action.
      replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { providerName: '', replies: [], requestedIds: [] } };
      notice = 'Message saved locally. Choose a provider and use Ask selected or Ask team to request a reply.';
    } catch (cause) {
      error = persisted ? `Message was saved, but the room could not refresh: ${String(cause)}` : `Could not confirm the message was saved: ${String(cause)}. Retrying the same text in this session will reuse its operation ID.`;
      if (!persisted && roomId === targetRoomId) draft = content;
    }
    finally { busy = false; }
  }

  function selectRoom(nextId: string) {
    if (nextId !== roomId) {
      draftsByRoom = { ...draftsByRoom, [roomId]: draft };
      draft = draftsByRoom[nextId] ?? '';
      roomId = nextId;
      messagePage = 0;
    }
    sidebarOpen = false;
  }

  function toggleAgent(id: string, checked: boolean) {
    const ids = checked ? [...new Set([...selectedAgentIds, id])] : selectedAgentIds.filter((selected) => selected !== id);
    selectedAgentsByRoom = { ...selectedAgentsByRoom, [roomId]: ids };
  }

  // The Rust command reports one outcome per agent. Missing/invalid outcomes
  // stay visibly unverified; never infer success from transport completion.
  function parseTeamReplies(value: unknown, room: string, providerId: string, requestedIds: string[]): TeamReply[] {
    if (typeof value !== 'object' || value === null) throw new Error('The team result was not recognized.');
    const response = value as TeamReplyResponse;
    if (response.room_id !== room || response.provider_id !== providerId || typeof response.prompt_message_id !== 'string' || !Array.isArray(response.results)) {
      throw new Error('The team result did not match the requested room/provider.');
    }
    const results: TeamReply[] = [];
    for (const entry of response.results) {
      if (typeof entry !== 'object' || entry === null) continue;
      const item = entry as Record<string, unknown>;
      if (typeof item.agent_id !== 'string' || !requestedIds.includes(item.agent_id)) continue;
      if (item.status !== 'succeeded' && item.status !== 'failed' && item.status !== 'skipped') continue;
      if (results.some((result) => result.agent_id === item.agent_id)) continue;
      results.push({ agent_id: item.agent_id, status: item.status, message_id: typeof item.message_id === 'string' ? item.message_id : undefined, reason: item.reason === 'already_answered' || item.reason === 'stale' ? item.reason : undefined, error: typeof item.error === 'string' ? item.error : undefined });
    }
    return results;
  }

  async function askAgents(mode: 'selected' | 'team') {
    if (!canAsk || !selectedProvider) return;
    const targetRoomId = roomId;
    const provider = selectedProvider;
    const ids = mode === 'selected' ? selectedAgentIds.slice(0, 1) : [...selectedAgentIds];
    replyBusy = true;
    error = '';
    replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { requestedIds: ids, providerName: provider.name, replies: [] } };
    try {
      const raw = await invoke<unknown>('studio_generate_team_replies', { roomId: targetRoomId, agentIds: ids, providerId: provider.id });
      const replies = parseTeamReplies(raw, targetRoomId, provider.id, ids);
      replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { requestedIds: ids, providerName: provider.name, replies } };
      await refresh();
    } catch (cause) {
      // A request can have partially succeeded before it failed. Retain that
      // uncertainty and refresh persisted messages without replaying the call.
      replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { requestedIds: ids, providerName: provider.name, replies: [], error: `Request failed or returned no usable status: ${String(cause)}. Check the room for saved replies before trying again.` } };
      try { await refresh(); } catch { /* keep original failure */ }
    } finally {
      replyBusy = false;
    }
  }

  async function copyTranscript() {
    if (!isNative || !roomId) return;
    try {
      const markdown = await invoke<string>('studio_export_room_markdown', { roomId });
      await navigator.clipboard.writeText(markdown);
      notice = 'Room transcript copied as Markdown. No file was written.';
    } catch (cause) {
      error = `Could not copy room transcript: ${String(cause)}`;
    }
  }

  async function saveProvider() {
    if (!isNative || !providerModel.trim()) return;
    working = true; error = '';
    try {
      await invoke('studio_save_provider', {
        name: providerName.trim(), endpoint: providerEndpoint.trim(), model: providerModel.trim(), apiKey: providerKey,
      });
      providerKey = ''; await refresh(); dialog = null;
      notice = 'Provider configured. API key is session-only; reconnection may be required after restart.';
    } catch (cause) { error = String(cause); }
    finally { working = false; }
  }

  function openTaskDialog(preferredRoomId?: string) {
    taskRoomId = preferredRoomId || taskFilterRoomId || roomId || snapshot.rooms[0]?.id || '';
    taskAgentId = (selectedAgentsByRoom[taskRoomId] ?? selectedAgentIds)[0] || snapshot.agents[0]?.id || '';
    taskInstructions = '';
    dialog = 'new-task';
  }

  async function createTask() {
    if (!isNative || working || !taskRoomId || !taskAgentId || !taskInstructions.trim()) return;
    const key = JSON.stringify([taskRoomId, taskAgentId, taskInstructions.trim()]);
    const ids = pendingTaskCreate?.key === key ? pendingTaskCreate : {
      key, taskId: crypto.randomUUID(), operationId: crypto.randomUUID()
    };
    pendingTaskCreate = ids;
    working = true; error = '';
    try {
      await invoke('studio_create_task', {
        roomId: taskRoomId, agentId: taskAgentId, instructions: taskInstructions.trim(),
        taskId: ids.taskId, operationId: ids.operationId
      });
      pendingTaskCreate = null;
      await refresh();
      dialog = null;
      screen = 'tasks';
      taskFilterRoomId = taskRoomId;
      notice = 'Task saved locally as pending. No agent execution has started.';
    } catch (cause) { error = `Could not save task: ${String(cause)}`; }
    finally { working = false; }
  }

  async function cancelTask(taskId: string) {
    if (!isNative || working) return;
    working = true; error = '';
    try {
      await invoke('studio_cancel_task', { taskId });
      await refresh();
      notice = 'Cancellation recorded. An already dispatched model request cannot be recalled.';
    } catch (cause) { error = `Could not cancel task: ${String(cause)}`; }
    finally { working = false; }
  }

  async function runTask(taskId: string) {
    if (!isNative || !selectedProviderId || runningTaskId || working) return;
    runningTaskId = taskId;
    error = '';
    try {
      await invoke('studio_run_task', { taskId, providerId: selectedProviderId });
      notice = 'Model task finished. Its response is saved in the associated room.';
    } catch (cause) {
      error = `Task was not completed: ${String(cause)}`;
    } finally {
      await refresh();
      runningTaskId = '';
    }
  }

  function keyboard(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); dialog = 'commands'; }
    if (event.key === 'Escape') { dialog = null; showDetails = false; }
  }

  function focusDialog(node: HTMLElement) {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const selector = 'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
    (node.querySelector<HTMLElement>('input:not([disabled]), select:not([disabled]), .command-list button') ?? node.querySelector<HTMLElement>(selector))?.focus();
    function trapTab(event: KeyboardEvent) {
      if (event.key !== 'Tab') return;
      const controls = Array.from(node.querySelectorAll<HTMLElement>(selector));
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    }
    node.addEventListener('keydown', trapTab);
    return { destroy() { node.removeEventListener('keydown', trapTab); previousFocus?.focus(); } };
  }

  function navigate(next: Screen) { screen = next; dialog = null; sidebarOpen = false; }
</script>

<svelte:window onkeydown={keyboard} />
<svelte:head><title>Constellation — Your AI team, in your space</title><meta name="description" content="A local-first workspace for collaborating with your AI team" /></svelte:head>

<div class="studio">
  <nav class="rail" aria-label="Main navigation">
    <div class="wordmark" title="Constellation"><div class="mark">✳</div></div>
    <div class="rail-items">
      <button class:active={screen === 'home'} onclick={() => navigate('home')} title="Home" aria-label="Home"><Activity size={20}/></button>
      <button class:active={screen === 'rooms'} onclick={() => navigate('rooms')} title="Rooms" aria-label="Rooms"><MessageCircle size={20}/></button>
      <button class:active={screen === 'tasks'} onclick={() => navigate('tasks')} title="Tasks" aria-label="Tasks"><Layers3 size={20}/></button>
      <button class:active={screen === 'library'} onclick={() => navigate('library')} title="Library" aria-label="Library"><BookOpen size={20}/></button>
    </div>
    <div class="rail-bottom">
      <button onclick={() => (dialog = 'commands')} title="Command palette" aria-label="Open command palette"><Command size={19}/></button>
      <button class:active={screen === 'settings'} onclick={() => navigate('settings')} title="Settings" aria-label="Settings"><Settings2 size={20}/></button>
      <div class="profile" title="Local workspace" aria-label="Local workspace">Y</div>
    </div>
  </nav>

  <aside class:open={sidebarOpen} class="sidebar" aria-label="Workspace sidebar">
    <div class="workspace-heading"><div class="workspace-icon">✧</div><div><strong>My workspace</strong><span>Personal space</span></div><ChevronDown size={15}/></div>
    {#if screen === 'rooms'}
      <div class="sidebar-section-title"><span>CONVERSATIONS</span><button aria-label="Create room" title="Create room" onclick={() => (dialog = 'new-room')}><Plus size={17}/></button></div>
      <div class="room-list">
        {#each snapshot.rooms as r (r.id)}
          <button class:selected={roomId === r.id} class="room-link" onclick={() => selectRoom(r.id)}><span class="room-symbol">#</span><span>{r.name}</span></button>
        {:else}
          <div class="sidebar-empty">Rooms you create appear here.</div>
        {/each}
      </div>
      <div class="sidebar-section-title agent-title"><span>YOUR TEAM</span><button title="Add an agent" aria-label="Add agent" onclick={() => (dialog = 'new-agent')}><Plus size={17}/></button></div>
      <div class="agent-list">
        {#each snapshot.agents as agent (agent.id)}
          <div class="agent-item"><div class="agent-avatar">✳</div><span>{agent.name}<small>{agent.role}</small></span><span class="agent-dot" title="Not running"></span></div>
        {:else}
          <div class="sidebar-empty">Build your team from the role library.</div>
        {/each}
      </div>
    {:else}
      <div class="sidebar-section-title"><span>WORKSPACE</span></div>
      <div class="section-context">{screen === 'home' ? 'A place to begin' : screen === 'tasks' ? 'Work in progress' : screen === 'library' ? 'People and capabilities' : 'Your preferences'}</div>
      <div class="sidebar-submenu"><button onclick={() => navigate('rooms')}><MessageCircle size={16}/> Back to rooms <ArrowRight size={15}/></button></div>
    {/if}
    <div class="sidebar-footer"><div class:inactive={!isNative} class="local-dot"></div><div><strong>{!runtimeChecked ? 'Checking local runtime' : isNative ? 'Workspace on this device' : 'Interface preview'}</strong><span>{!runtimeChecked ? 'Checking availability…' : isNative ? 'Local app — no operator cloud' : 'Desktop app needed to save'}</span></div><ShieldCheck size={16}/></div>
  </aside>

  <main class="main">
    {#if runtimeChecked && !isNative}<div class="preview-banner" role="status"><Cpu size={16}/> Browser preview: explore the interface here. Create, save, and send actions require the installed desktop app.</div>{/if}
    {#if notice}<div class="toast" role="status"><Check size={15}/>{notice}<button aria-label="Dismiss notice" onclick={() => (notice = '')}><X size={14}/></button></div>{/if}
    {#if error}<div class="error-toast" role="alert">{error}<button aria-label="Dismiss error" onclick={() => (error = '')}><X size={15}/></button></div>{/if}

    {#if screen === 'rooms'}
      <header class="topbar">
        <button class="mobile-menu icon-button" aria-label="Show rooms" aria-expanded={sidebarOpen} onclick={() => (sidebarOpen = !sidebarOpen)}><Menu size={20}/></button>
        <div class="crumb"><span class="crumb-muted">Rooms</span><ChevronRight size={15}/><strong>{room?.name || 'Your first space'}</strong></div>
        <div class="top-actions"><span class="privacy-note"><ShieldCheck size={14}/> Local by default</span>{#if room}<button class="subtle-cta room-task-action" onclick={() => openTaskDialog(roomId)}><Plus size={15}/> Plan task</button>{/if}<button class="icon-button" aria-label="Room details" aria-expanded={showDetails} title="Room details" onclick={() => (showDetails = !showDetails)}><UsersRound size={19}/></button><button class="icon-button" aria-label="More room options" title="More room options" onclick={() => (dialog = 'commands')}><MoreHorizontal size={19}/></button></div>
      </header>
      <div class="conversation-area">
        {#if !room}
          <div class="empty-chat">
            <div class="hero-symbol">✳<span>✦</span></div>
            <div class="eyebrow">YOUR SPACE FOR BETTER WORK</div>
            <h1>Good ideas are<br /><em>better together.</em></h1>
            <p>Bring your favorite AI models into one thoughtful space. Ask a question, build a team, or get something done — on your terms.</p>
            <div class="hero-actions"><button class="primary-button" onclick={() => (dialog = 'new-room')}><Plus size={17}/> Create your first room</button><button class="soft-button" onclick={() => navigate('library')}>Explore roles <ArrowRight size={16}/></button></div>
            <div class="hero-footnote"><ShieldCheck size={15}/> Your files and tasks stay on your device unless you approve otherwise.</div>
          </div>
        {:else}
          <div class="chat-scroll" role="log" aria-label="Conversation history" aria-live="off">
            <div class="room-intro"><div class="room-intro-icon">#</div><h1>{room.name}</h1><p>{room.description || 'A space for you and your AI teammates to collaborate.'}</p><button class="soft-button small" onclick={() => (dialog = 'new-agent')}><Plus size={15}/> Add a teammate</button></div>
            {#if messages.length > messagePageSize}
              <div class="message-pagination" role="group" aria-label="Conversation message pages">
                <button type="button" class="soft-button small" disabled={messagePage >= maxMessagePage} onclick={() => (messagePage = Math.min(maxMessagePage, messagePage + 1))}>Older messages</button>
                <span>Showing {Math.max(1, visibleMessageEnd - visibleMessages.length + 1)}–{visibleMessageEnd} of {messages.length}</span>
                <button type="button" class="soft-button small" disabled={messagePage === 0} onclick={() => (messagePage = Math.max(0, messagePage - 1))}>Newer messages</button>
              </div>
            {/if}
            {#each visibleMessages as message (message.id)}
              <article class="message"><div class:user-avatar={message.sender_kind === 'user'} class="message-avatar">{message.sender_kind === 'user' ? 'Y' : '✳'}</div><div class="message-body"><div class="message-byline"><strong>{message.sender_name}</strong><span>{timeDisplay(message.created_at)}</span>{#if message.simulated}<span class="simulated">Simulation</span>{/if}</div><div class="message-text">{message.body}</div></div></article>
            {/each}
          </div>
          <div class="composer-container">
            <form class="composer" onsubmit={(event) => {event.preventDefault(); void sendMessage();}}>
              <textarea bind:value={draft} disabled={busy || replyBusy || !isNative} aria-label="Write a local message" placeholder="Write a message to this room..." rows="2" onkeydown={(event) => {if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {event.preventDefault(); void sendMessage();}}}></textarea>
              <div class="compose-bottom"><span class="save-local-label"><ShieldCheck size={14}/> Save in this room</span><button class="send-button" type="submit" disabled={!draft.trim() || busy || replyBusy || !isNative} title="Save message locally" aria-label="Save message locally">{#if busy}<Square size={16}/>{:else}<ArrowRight size={19}/>{/if}</button></div>
            </form>
            <div class="reply-panel" aria-label="Request AI replies">
              <div class="reply-row">
                <label class="reply-provider">Model provider
                  <select bind:value={selectedProviderId} disabled={!isNative || busy || replyBusy || snapshot.providers.length === 0} aria-label="Choose AI provider for replies">
                    <option value="">Choose a provider…</option>
                    {#each snapshot.providers as provider (provider.id)}<option value={provider.id}>{provider.name} · {provider.model}</option>{/each}
                  </select>
                </label>
                <div class="reply-actions"><button type="button" class="soft-button small" disabled={!canAsk} onclick={() => void askAgents('selected')}>Ask selected</button><button type="button" class="primary-button small" disabled={!canAsk || selectedAgentIds.length > 8} onclick={() => void askAgents('team')}>Ask team ({selectedAgentIds.length})</button></div>
              </div>
              {#if snapshot.agents.length > 0}
                <details class="reply-agent-picker"><summary>Agents · {selectedAgentIds.length} selected <span>Change team</span></summary>
                  <fieldset class="reply-agents" disabled={!isNative || busy || replyBusy}>
                    <legend>Choose one or more agents</legend>
                    <div class="agent-options">{#each snapshot.agents as agent (agent.id)}<label class:chosen={selectedAgentIds.includes(agent.id)}><input type="checkbox" checked={selectedAgentIds.includes(agent.id)} onchange={(event) => toggleAgent(agent.id, event.currentTarget.checked)}/><span>{agent.name}</span></label>{/each}</div>
                  </fieldset>
                </details>
                <p class="reply-help">Ask selected contacts the first checked agent. Ask team contacts every checked agent (up to 8). The latest saved message and room context go only to your chosen provider.</p>
              {:else}<p class="reply-help">No agents yet. <button type="button" class="text-button" onclick={() => (dialog = 'new-agent')}>Add a teammate</button> before requesting AI replies.</p>{/if}
              {#if snapshot.providers.length === 0}<p class="reply-help">No model providers configured. <button type="button" class="text-button" onclick={() => (dialog = 'provider')}>Connect a model</button> to ask your team.</p>{/if}
              {#if selectedAgentIds.length > 8}<p class="reply-warning" role="status">Choose at most 8 agents for one team request.</p>{/if}
              {#if replyBusy}<p class="reply-help" role="status">Requesting replies from {selectedProvider?.name ?? 'the chosen provider'}… Each agent may have a different outcome.</p>{/if}
              {#if replyFeedbackByRoom[roomId] && replyFeedbackByRoom[roomId].requestedIds.length > 0}
                {@const report = replyFeedbackByRoom[roomId]}
                <div class="reply-results" aria-label="Latest AI reply results" role="status"><strong>Latest request · {report.providerName}</strong>
                  {#if report.error}<p class="reply-warning">{report.error}</p>{/if}
                  <ul>{#each report.requestedIds as id (id)}
                    {@const outcome = report.replies.find((reply) => reply.agent_id === id)}
                    <li><span>{snapshot.agents.find((agent) => agent.id === id)?.name ?? 'Unknown agent'}</span><span class:reply-good={outcome?.status === 'succeeded'} class:reply-warning={outcome?.status === 'failed'}>{outcome?.status === 'succeeded' ? 'Replied' : outcome?.status === 'failed' ? 'Failed' : outcome?.status === 'skipped' ? `Skipped${outcome.reason ? ` (${outcome.reason.replace('_', ' ')})` : ''}` : 'No confirmed result'}{#if outcome?.error} — {outcome.error}{/if}</span></li>
                  {/each}</ul>
                </div>
              {/if}
              <p class="composer-footnote">{!isNative ? 'Browser preview only — open the installed desktop app.' : snapshot.providers.length === 0 ? 'Local messages work without a model.' : !selectedProvider ? 'Choose a provider to explicitly allow a model request.' : !messages.some((m) => m.sender_kind === 'user') ? 'Save a message locally before requesting a reply.' : 'AI requests may send private room context to the selected provider. Already answered agents are skipped on retries.'}</p>
            </div>
          </div>
        {/if}
      </div>
    {:else if screen === 'home'}
      <header class="topbar"><div class="crumb"><strong>Home</strong></div><span class="privacy-note"><ShieldCheck size={14}/> Your own space</span></header>
      <div class="content-page"><div class="eyebrow">WELCOME TO CONSTELLATION</div><h1>What will we<br /><em>work on today?</em></h1><p class="page-lede">Start a focused conversation, assemble a small team, and stay in control of every action.</p><div class="action-grid"><button onclick={() => (dialog = 'new-room')}><MessageCircle size={22}/><strong>Start a conversation</strong><span>Make room for a new idea</span><ArrowRight size={18}/></button><button onclick={() => navigate('library')}><UsersRound size={22}/><strong>Find a specialist</strong><span>Choose a helpful AI role</span><ArrowRight size={18}/></button><button onclick={() => (dialog = 'provider')}><Cpu size={22}/><strong>Connect a model</strong><span>Use a remote API or local server</span><ArrowRight size={18}/></button></div><div class="home-status"><div><div class="local-dot"></div> Your workspace stays on this computer by default</div><span>{snapshot.rooms.length} rooms · {snapshot.agents.length} agents · {snapshot.providers.length} providers</span></div></div>
    {:else if screen === 'tasks'}
      <header class="topbar"><div class="crumb"><strong>Tasks</strong></div><button class="subtle-cta" onclick={() => openTaskDialog()}><Plus size={16}/> Plan task</button></header>
      <div class="content-page"><div class="eyebrow">WORK, WITHOUT THE GUESSWORK</div><h1>Every step,<br /><em>in the open.</em></h1><p class="page-lede">Plan work for your agents, then explicitly run a text-only task through a model you choose. Results are recorded in the room. The app cannot yet execute coding tools or other device actions.</p>
        <label class="task-provider-selector">Model for task execution
          <select bind:value={selectedProviderId} disabled={!isNative || !!runningTaskId}>
            <option value="">Choose a provider (required)…</option>
            {#each snapshot.providers as provider (provider.id)}<option value={provider.id}>{provider.name} · {provider.model} · {provider.endpoint}</option>{/each}
          </select>
        </label>
        <p class="task-provider-notice">Pressing Run sends the task instructions to this provider. No device tools are available to the model. You may cancel the local result, but an HTTP request already sent cannot be recalled.</p>
        <div class="tasks-filter"><label for="tasks-room-filter">Show tasks for</label><select id="tasks-room-filter" bind:value={taskFilterRoomId}><option value="">All rooms</option>{#each snapshot.rooms as taskRoom (taskRoom.id)}<option value={taskRoom.id}>{taskRoom.name}</option>{/each}</select></div>
        {#if roomTasks.length === 0}
          <div class="blank-panel"><div class="blank-icon"><Layers3 size={26}/></div><h2>{taskFilterRoomId ? 'No tasks in this room' : 'No tasks planned yet'}</h2><p>Create a room and a teammate, then save a task. This build records plans locally and does not execute them.</p><button class="soft-button" onclick={() => openTaskDialog()}>Plan your first task <ArrowRight size={15}/></button></div>
        {:else}
          <div class="task-list" aria-label="Saved tasks">
            {#each roomTasks as task (task.id)}
              <article class="task-card">
                <div class="task-card-top"><span class="task-state" data-state={task.state}>{task.state === 'pending' ? 'Pending · Not executed' : task.state === 'cancelled' ? 'Cancelled' : task.state === 'running' ? 'Running · Text request' : task.state}</span><span class="task-date">{timeDisplay(task.created_at)}</span></div>
                <p class="task-instructions">{task.instructions}</p>
                <div class="task-card-bottom"><span>{snapshot.agents.find((agent) => agent.id === task.agent_id)?.name || 'Unknown agent'} · {snapshot.rooms.find((room) => room.id === task.room_id)?.name || 'Unknown room'}</span>
                  {#if task.state === 'pending'}
                    <button class="primary-button small" disabled={!isNative || working || !!runningTaskId || !selectedProviderId} onclick={() => void runTask(task.id)}>{runningTaskId === task.id ? 'Running…' : 'Run text task'}</button>
                  {/if}
                  {#if task.state === 'pending' || task.state === 'queued' || task.state === 'running'}
                    <button class="soft-button small" disabled={!isNative || working} onclick={() => void cancelTask(task.id)}>Cancel task</button>
                  {/if}
                </div>
                {#if task.failure_reason}<p class="task-failure">{task.failure_reason}</p>{/if}
              </article>
            {/each}
          </div>
        {/if}
      </div>
    {:else if screen === 'library'}
      <header class="topbar"><div class="crumb"><strong>Role library</strong></div><button class="subtle-cta" onclick={() => (dialog = 'new-agent')}><Plus size={16}/> Create agent</button></header>
      <div class="content-page library-page"><div class="eyebrow">THE RIGHT TEAM FOR ANY IDEA</div><h1>Find your <em>people.</em></h1><p class="page-lede">Browse curated roles. Each role provides tailored guidance when chatting with a model, but no tools, filesystem access, or execution privileges are granted.</p><label class="searchbox"><Search size={18}/><input aria-label="Search agent roles" bind:value={search} placeholder="Search roles..."/></label><div class="roles-grid">{#each filteredRoles as role (role.name)}<div class="role-card"><div class="role-icon">{role.icon}</div><div class="role-group">{role.group}</div><h2>{role.name}</h2><p>{role.blurb}</p><button aria-label={`Create ${role.name} agent`} disabled={!isNative || working} title={!isNative ? 'Available in the installed desktop app' : 'Add to your team'} onclick={() => void createAgent(role.name, role.name)}>Add to team <Plus size={16}/></button></div>{:else}<p>No roles match your search.</p>{/each}</div><p class="library-disclaimer">{starterRoles.length} distinct prompt-only roles. Fully operational tool-enabled agents are still being developed and no tools are granted by selecting a role.</p></div>
    {:else}
      <header class="topbar"><div class="crumb"><strong>Settings</strong></div></header>
      <div class="content-page settings-page"><div class="eyebrow">MAKE IT YOURS</div><h1>Your space.<br /><em>Your rules.</em></h1><p class="page-lede">Use any compatible AI provider, including one running locally. No account needed to open your workspace.</p><div class="settings-block"><div><Cpu size={23}/><div><h2>AI connections</h2><p>Connect models to the agents you choose.</p></div></div><button class="primary-button" onclick={() => (dialog = 'provider')}><Plus size={16}/> Add provider</button></div><div class="provider-list">{#each snapshot.providers as provider (provider.id)}<div class="provider-row"><div class="provider-symbol"><Cpu size={18}/></div><div><strong>{provider.name}</strong><span>{provider.model} · {provider.endpoint}</span></div><span class="provider-status">Configured</span></div>{:else}<div class="provider-none">No AI providers are configured yet.</div>{/each}</div><button class="advanced-toggle" aria-expanded={advanced} onclick={() => (advanced = !advanced)}><SlidersHorizontal size={17}/> Advanced Studio <ChevronDown size={16}/></button>{#if advanced}<div class="advanced-panel"><h2>Advanced features</h2><p>Granular tool permissions, runner pairing, encrypted synchronization, and workflow controls are in development. This build does not claim these protections are active.</p></div>{/if}</div>
    {/if}
  </main>

  {#if showDetails && screen === 'rooms'}<aside class="details" aria-label="Room information"><div class="details-heading"><strong>Room details</strong><button class="icon-button" aria-label="Close details" onclick={() => (showDetails = false)}><X size={18}/></button></div><div class="details-section"><span class="eyebrow">CURRENT SPACE</span><h2>{room?.name || 'No room selected'}</h2><p>{room?.description || 'Bring people and agents together to work.'}</p>{#if room}<button class="soft-button small" disabled={!isNative} onclick={() => void copyTranscript()}>Copy transcript</button>{/if}</div><div class="details-section"><span class="eyebrow">EXECUTION</span><div class="details-row"><Cpu size={18}/> This device</div><p>There is no remote runner configured.</p></div><div class="details-section"><span class="eyebrow">CONNECTED AGENTS</span><p>{snapshot.agents.length} available teammates</p><button class="soft-button small" onclick={() => (dialog = 'new-agent')}><Plus size={14}/> Add agent</button></div></aside>{/if}
</div>

{#if dialog}
  <div class="modal-backdrop">
    <div class="modal" role="dialog" aria-modal="true" aria-label={dialog === 'new-room' ? 'Create room' : dialog === 'new-agent' ? 'Create agent' : dialog === 'new-task' ? 'Plan task' : dialog === 'provider' ? 'Connect AI provider' : 'Command palette'} use:focusDialog>
      <div class="modal-heading"><div class="modal-icon">{#if dialog === 'new-room'}<MessageCircle size={21}/>{:else if dialog === 'new-agent'}<Bot size={21}/>{:else if dialog === 'new-task'}<Layers3 size={21}/>{:else if dialog === 'provider'}<Cpu size={21}/>{:else}<Command size={21}/>{/if}</div><button class="icon-button" aria-label="Close dialog" onclick={() => (dialog = null)}><X size={20}/></button></div>
      {#if dialog === 'new-room'}<h2>A space for what’s next.</h2><p>Start with a room name. Invite agents when you're ready.</p><form onsubmit={(event) => {event.preventDefault();void createRoom();}}><label>ROOM NAME<input bind:value={newRoom} placeholder="e.g. Research lab, My next product" maxlength="80" required/></label><button class="primary-button full" disabled={working || !newRoom.trim() || !isNative} type="submit">Create room <ArrowRight size={17}/></button></form>
      {:else if dialog === 'new-agent'}<h2>Meet your next teammate.</h2><p>Choose a role and a name. Permissions and model capabilities still determine what an agent can actually do.</p><form onsubmit={(event) => {event.preventDefault();void createAgent(selectedRole,newAgent);}}><label>ROLE<select bind:value={selectedRole}>{#each starterRoles as role}<option value={role.name}>{role.name}</option>{/each}</select></label><label>DISPLAY NAME<input bind:value={newAgent} placeholder="e.g. Avery" maxlength="80"/></label><button class="primary-button full" disabled={working || !isNative} type="submit">Add teammate <ArrowRight size={17}/></button></form>
      {:else if dialog === 'new-task'}<h2>Plan your next task.</h2><p>Save instructions for a teammate. Execution and tool permissions will be available in a future build.</p><form onsubmit={(event) => {event.preventDefault();void createTask();}}><label>ROOM<select bind:value={taskRoomId} required>{#each snapshot.rooms as r (r.id)}<option value={r.id}>{r.name}</option>{/each}</select></label><label>TEAMMATE<select bind:value={taskAgentId} required>{#each snapshot.agents as agent (agent.id)}<option value={agent.id}>{agent.name} — {agent.role}</option>{/each}</select></label><label>INSTRUCTIONS<textarea bind:value={taskInstructions} rows="4" maxlength="10000" required placeholder="Describe the outcome you want..."></textarea></label><button class="primary-button full" type="submit" disabled={!isNative || working || !taskRoomId || !taskAgentId || !taskInstructions.trim()}>Save pending task <ArrowRight size={17}/></button></form>{#if !snapshot.rooms.length || !snapshot.agents.length}<p class="dialog-footnote">Create a room and add a teammate before saving a task.</p>{/if}
      {:else if dialog === 'provider'}<h2>Bring your own model.</h2><p>Use an HTTPS API endpoint or a trusted localhost model server. Credentials are stored for this session only in this initial build.</p><form onsubmit={(event) => {event.preventDefault();void saveProvider();}}><label>DISPLAY NAME<input bind:value={providerName} required /></label><label>API BASE URL<input bind:value={providerEndpoint} placeholder="https://api.example.com/v1" required /></label><label>MODEL IDENTIFIER<input bind:value={providerModel} placeholder="e.g. llama3.2, gpt-4o-mini" required /></label><label>API KEY (OPTIONAL FOR LOCAL)<input type="password" autocomplete="off" bind:value={providerKey} placeholder="Not written to disk" /></label><button class="primary-button full" disabled={working || !providerModel.trim() || !isNative} type="submit">Save connection <ArrowRight size={17}/></button></form><p class="dialog-footnote"><ShieldCheck size={14}/> Check the provider's privacy policy before sending private content.</p>
      {:else}<h2>Where would you like to go?</h2><p>Quickly navigate your workspace. <kbd>Esc</kbd> closes this menu.</p><div class="command-list">{#each [{label:'Home',target:'home'},{label:'Rooms',target:'rooms'},{label:'Tasks',target:'tasks'},{label:'Role library',target:'library'},{label:'Settings',target:'settings'}] as command}<button onclick={() => navigate(command.target as Screen)}>{command.label}<ArrowRight size={16}/></button>{/each}</div>{/if}
      {#if !isNative}<p class="dialog-footnote" role="status">This is a browser preview. Changes require the installed desktop app.</p>{/if}
    </div>
  </div>
{/if}
