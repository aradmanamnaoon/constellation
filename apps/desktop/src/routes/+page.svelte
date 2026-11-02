<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import {
    Activity, ArrowRight, Bot, Check, ChevronDown,
    ChevronRight, Command, Layers3,
    Menu, MessageCircle, MoreHorizontal, Plus, Search, Settings2,
    ShieldCheck, SlidersHorizontal, Sparkles, Square, UsersRound, X,
    Cpu, BookOpen, FileSearch, Maximize2, Minimize2,
  } from '@lucide/svelte';
  import roleCatalog from '../../../../catalog/roles.json';
  import WorkspaceCommandPalette from '../lib/WorkspaceCommandPalette.svelte';
  import { filterWorkspaceRooms, type WorkspaceAction } from '../lib/workspace-navigation';
  import { filterRoleLibrary, roleCategories } from '../lib/role-library';
  import { splitChatBlocks } from '../lib/chat-format';
  import { filterProviderDirectory, type ProviderStatusFilter } from '../lib/provider-directory';
  import { prepareProviderRevision } from '../lib/provider-revision';
  import { paginateLocal } from '../lib/local-pagination';
  import { DraftPersistence, type DraftState, type StoredDraft } from '../lib/draft-persistence';
  import './studio.css';

  type Screen = 'home' | 'rooms' | 'tasks' | 'evidence' | 'library' | 'settings';
  type Room = { id: string; name: string; description: string; member_ids: string[]; members_configured?: boolean };
  type Agent = { id: string; name: string; role: string; provider_id?: string | null };
  type ChatMessage = { id: string; room_id: string; sender_kind: 'user' | 'agent' | 'system'; sender_name: string; body: string; created_at: number | string; simulated?: boolean };
  type ProviderKind = 'openai_chat' | 'openai_responses' | 'anthropic_messages' | 'google_gemini' | 'ollama_native' | 'custom_compatible';
  type Provider = { id: string; name: string; endpoint: string; model: string; connected: boolean; kind?: ProviderKind; remembered_key?: boolean };
  type ProviderProbe = { provider_id: string; status: 'compatible_text' | 'invalid_configuration' | 'unreachable' | 'unauthorized' | 'invalid_response' | 'unsupported_text' | 'rate_limited' | 'provider_error'; http_status?: number | null; text_chat_probe: boolean; tools_verified: boolean; streaming_verified: boolean };
  type StudioTask = { id: string; room_id: string; agent_id: string; instructions: string; state: 'pending' | 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'; failure_reason: string | null; created_at: number; updated_at: number };
  type TeamReply = { agent_id: string; status: 'succeeded' | 'failed' | 'skipped'; message_id?: string; reason?: 'already_answered' | 'stale'; error?: string };
  type TeamReplyResponse = { room_id: string; prompt_message_id: string; provider_id: string; results: unknown };
  type StreamReplyDelta = { room_id: string; agent_id: string; delta: string; ready: boolean };
  type ReplyFeedback = { providerName: string; replies: TeamReply[]; requestedIds: string[]; error?: string };
  type PendingSend = { content: string; messageId: string; operationId: string };
  type EvidenceHit = { source_id: string; relative_path: string; source_sha256: string; start_byte: number; end_byte: number; quote: string; bm25_score: number };
  type EvidenceImportAnchor = { start_byte: number; end_byte: number; kind: 'pdf_page' | 'docx_paragraph'; number: number };
  type SavedEvidence = { source_id: string; relative_path: string; source_sha256: string; title: string; bytes: number; format: string };
  type SavedEvidenceDetail = SavedEvidence & { text: string };
  type WorkflowPreview = { workflow_id: string; ordered_step_ids: string[]; ready_step_ids: string[]; step_count: number; preview_only: boolean; provider_requests_sent: boolean; tools_executed: boolean; stored: boolean };
  type WorkflowStepDraft = { id: string; title: string; agentId: string; providerId: string; instruction: string };
  type WorkflowStepDefinition = { id: string; role: string; agent_id: string; provider_id: string; instruction: string; depends_on: string[]; max_attempts: number };
  type WorkflowDefinition = { schema_version: number; workflow_id: string; steps: WorkflowStepDefinition[] };
  type SavedWorkflow = { definition: WorkflowDefinition; states: Record<string, { status: string; attempts: number; result_text?: string | null }>; ready_step_ids: string[]; revision: number; cancelled: boolean };
  type WorkflowListResponse = { items: SavedWorkflow[]; max_items: number; offset: number; query: string; has_more: boolean };
  type WorkflowCreateResponse = { workflow_id: string; revision: number; ready_step_ids: string[]; stored: boolean; executed: boolean; tools_executed: boolean };
  type WorkflowCancelResponse = { workflow_id: string; revision: number; cancelled: boolean };
  type WorkflowStepResponse = { workflow_id: string; step_id: string; revision: number; status: string; result_text: string; ready_step_ids: string[]; provider_requests_sent: boolean; tools_executed: boolean };
  type WorkflowRunResponse = { workflow_id: string; revision: number; executed_step_ids: string[]; finished: boolean; provider_requests_sent: boolean; tools_executed: boolean };
  // `digest` is an opaque, one-use approval ticket. The native dialog alone
  // receives the private content hash and original operation commitment.
  type FileToolPreview = { operation_id: string; digest: string; action: 'read' | 'create_new'; canonical_path: string; byte_count?: number; sha256?: string; reviewed_content?: string; task_id: string; permission_granted: false; origin?: 'model_proposal' };
  type FileToolResult = { operation_id: string; status: 'succeeded' | 'failed'; executed: true; bytes: number; sha256: string | null; content: string | null; error: string | null; receipt_digest: string; model_observation_authenticated?: boolean; automatic_resume_allowed?: false };
  type FileToolDenial = { operation_id: string; status: 'rejected'; executed: false; reason: string };
  type ProposalUsage = { task_used: number; provider_used: number; task_limit: number; provider_limit: number };
  type ModelRecoveryStatus = 'not_in_journal' | 'approved_not_started' | 'denied' | 'uncertain' | 'completed';
  type ModelRecoveryOutcome = 'succeeded' | 'failed' | 'cancelled' | 'timed_out' | 'output_limit';
  type ModelRecoveryOperation = { operation_id: string; checkpoint_revision: number; status: ModelRecoveryStatus; outcome: ModelRecoveryOutcome | null };
  type ModelRecovery = { task_id: string; original_provider_id: string | null; automatic_resume_allowed: false; native_approval_restored: false; operations: ModelRecoveryOperation[] };
  type MultiStepState = {
    status: 'not_started' | 'awaiting_model' | 'awaiting_native_review' | 'complete' | 'cancelled' | 'quarantined' | 'provider_failed';
    phase?: 'settled' | 'provider_in_flight' | 'native_review_required';
    revision?: number; provider_id?: string; model_reservations?: number; tool_steps?: number;
    model_limit?: number; tool_limit?: number; native_review_busy?: boolean;
    action?: 'read' | 'create_new' | null; relative_path?: string | null;
  };
  type Snapshot = { rooms: Room[]; agents: Agent[]; messages: ChatMessage[]; providers: Provider[]; tasks: StudioTask[] };
  type Dialog = null | 'new-room' | 'new-agent' | 'new-task' | 'provider' | 'commands';

  const maxEvidenceDocumentBytes = 128 * 1024;
  const maxEvidenceQueryBytes = 2048;
  const maxBackupPassphraseBytes = 1024;
  const maxRecoveryPathBytes = 4096;
  const utf8Bytes = (value: string): number => new TextEncoder().encode(value).length;

  const starterRoles = roleCatalog.roles.map((role) => ({ name: role.name, icon: '✳', group: role.category, blurb: role.summary }));
  // Display presets only. Actual transport/routing is exclusively handled by Rust.
  const providerPresets: Record<ProviderKind, { label: string; name: string; endpoint: string; modelHint: string; keyHint: string; detail: string }> = {
    openai_chat: {
      label: 'OpenAI · Chat Completions', name: 'OpenAI Chat', endpoint: 'https://api.openai.com/v1',
      modelHint: 'Your Chat Completions model ID', keyHint: 'OpenAI API key',
      detail: 'OpenAI Chat Completions API. Requires a compatible model and API credentials.'
    },
    openai_responses: {
      label: 'OpenAI · Responses', name: 'OpenAI Responses', endpoint: 'https://api.openai.com/v1',
      modelHint: 'Your Responses model ID', keyHint: 'OpenAI API key',
      detail: 'OpenAI Responses API uses a different request format from Chat Completions.'
    },
    anthropic_messages: {
      label: 'Anthropic · Messages', name: 'Anthropic', endpoint: 'https://api.anthropic.com/v1',
      modelHint: 'Your Claude model ID', keyHint: 'Anthropic API key',
      detail: 'Native Anthropic Messages format; not an OpenAI-compatible endpoint.'
    },
    google_gemini: {
      label: 'Google · Gemini', name: 'Google Gemini', endpoint: 'https://generativelanguage.googleapis.com/v1beta',
      modelHint: 'Your Gemini model ID', keyHint: 'Gemini API key',
      detail: 'Native Google Gemini generateContent format; model and account availability vary.'
    },
    ollama_native: {
      label: 'Ollama · Native local API', name: 'Local Ollama', endpoint: 'http://127.0.0.1:11434',
      modelHint: 'Installed Ollama model name', keyHint: 'Usually no key for local Ollama',
      detail: 'Native Ollama HTTP API (not its OpenAI-compatible /v1 route). Ollama must be running on this device.'
    },
    custom_compatible: {
      label: 'Custom · OpenAI-compatible', name: 'Custom provider', endpoint: 'http://127.0.0.1:11434/v1',
      modelHint: 'Model ID accepted by your endpoint', keyHint: 'Optional for trusted local servers',
      detail: 'OpenAI Chat Completions-compatible server. Compatibility is not guaranteed; check your provider documentation.'
    }
  };

  function providerKindLabel(kind?: string): string {
    if (!kind) return 'Legacy OpenAI-compatible';
    return Object.prototype.hasOwnProperty.call(providerPresets, kind) ? providerPresets[kind as ProviderKind].label : 'Unknown provider type';
  }

  let screen = $state<Screen>('rooms');
  let dialog = $state<Dialog>(null);
  let snapshot = $state<Snapshot>({ rooms: [], agents: [], messages: [], providers: [], tasks: [] });
  let roomId = $state('');
  let draft = $state('');
  let draftsByRoom = $state<Record<string, string>>({});
  let draftReady = $state(false);
  let draftLoadError = $state('');
  let draftStates = $state<Record<string, DraftState>>({});
  let draftErrors = $state<Record<string, string>>({});
  const draftPersistence = new DraftPersistence(
    (id, content, expectedRevision) => invoke<StoredDraft>('studio_save_room_draft', {
      roomId: id, content, expectedRevision,
    }),
    (id, status, saveError) => {
      draftStates = { ...draftStates, [id]: status };
      draftErrors = { ...draftErrors, [id]: saveError ?? '' };
    },
  );
  let pendingSendsByRoom = $state<Record<string, PendingSend>>({});
  let selectedProviderId = $state('');
  let selectedAgentsByRoom = $state<Record<string, string[]>>({});
  let replyBusy = $state(false);
  let replyCancelBusy = $state(false);
  let streamingRoomId = $state('');
  let streamStopRequested = $state(false);
  let replyFeedbackByRoom = $state<Record<string, ReplyFeedback>>({});
  let streamAnswers = $state(false);
  let themeMode = $state<'system' | 'light' | 'dark'>('system');
  let streamPreviewByRoom = $state<Record<string, Record<string, string>>>({});
  let newRoom = $state('');
  let newAgent = $state('');
  let selectedRole = $state(starterRoles[0]?.name ?? '');
  let search = $state('');
  let roleCategory = $state('All');
  let rolePageSize = $state(24);
  let roomSearch = $state('');
  let providerName = $state('Custom provider');
  let providerKind = $state<ProviderKind>('custom_compatible');
  let providerEndpoint = $state('http://127.0.0.1:11434/v1');
  let providerModel = $state('');
  let providerKey = $state('');
  let providerRememberKey = $state(false);
  let providerRevisionSourceId = $state('');
  let providerRevisionSavedKey = $state(false);
  let providerRemovalCandidate = $state('');
  let providerForgetCandidate = $state('');
  let providerForgetConfirmation = $state('');
  let probeBusyId = $state('');
  let probeResults = $state<Record<string, ProviderProbe | { status: 'request_error'; message: string }>>({});
  let providerSearch = $state('');
  let providerStatusFilter = $state<ProviderStatusFilter>('all');
  let taskRoomId = $state('');
  let taskFilterRoomId = $state('');
  let taskSearch = $state('');
  let taskStatusFilter = $state<'all' | 'open' | 'done' | 'attention'>('all');
  const taskPageSize = 40;
  let taskPage = $state(0);
  let taskProviders = $state<Record<string, string>>({});
  let taskAgentId = $state('');
  let taskInstructions = $state('');
  let taskSetupReturn = $state<'new-room' | 'new-agent' | null>(null);
  let tasksView = $state<'tasks' | 'workflows'>('tasks');
  let runningTaskId = $state('');
  let taskCancelConfirmation = $state('');
  let fileToolTaskId = $state('');
  let fileToolAction = $state<'read' | 'create_new'>('create_new');
  let fileToolPath = $state('');
  let fileToolContent = $state('');
  let fileToolBusy = $state(false);
  let fileToolPreview = $state<FileToolPreview | null>(null);
  let fileToolResult = $state<FileToolResult | null>(null);
  let fileToolError = $state('');
  let fileProposalText = $state('');
  let fileToolOriginProviderId = $state('');
  let finalFollowupError = $state('');
  let fileProposalUsage = $state<(ProposalUsage & { taskId: string; providerId: string }) | null>(null);
  let recoveryInspectionTaskId = $state('');
  let recoveryInspections = $state<Record<string, ModelRecovery>>({});
  let recoveryInspectionErrors = $state<Record<string, string>>({});
  let recoveryInspectionSequence = 0;
  let multiStepStates = $state<Record<string, MultiStepState>>({});
  let multiStepProviders = $state<Record<string, string>>({});
  let multiStepBusyId = $state('');
  let multiStepErrors = $state<Record<string, string>>({});
  let multiStepNotices = $state<Record<string, string>>({});
  let multiStepMaxModelRequests = $state(6);
  let multiStepMaxToolActions = $state(3);
  let pendingTaskCreate = $state<{ key: string; taskId: string; operationId: string } | null>(null);
  let working = $state(false);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let sidebarOpen = $state(false);
  let showDetails = $state(false);
  let focusMode = $state(false);
  let advanced = $state(false);
  let evidenceText = $state('');
  let evidenceQuery = $state('');
  let evidenceHits = $state<EvidenceHit[]>([]);
  let evidenceSearched = $state(false);
  let evidenceBusy = $state(false);
  let evidenceError = $state('');
  let evidenceSaveTitle = $state('');
  let evidenceSaving = $state(false);
  let evidenceSavedSources = $state<SavedEvidence[]>([]);
  let evidenceInventoryLoaded = $state(false);
  let evidenceInventoryError = $state('');
  let evidenceNotice = $state('');
  let evidenceLoadConfirmation = $state<{sourceId: string; sha256: string} | null>(null);
  let evidenceSavedResults = $state(false);
  let evidenceSavedOffset = $state(0);
  let evidenceSavedHasMore = $state(false);
  let evidenceDeleteConfirmation = $state<{sourceId: string; sha256: string} | null>(null);
  let evidenceImportRights = $state(false);
  let evidencePendingImport = $state<File | null>(null);
  let evidenceImportBusy = $state(false);
  let evidenceImportInfo = $state('');
  let evidenceImportAnchors = $state<EvidenceImportAnchor[]>([]);
  let evidenceImportSourceSha = $state('');
  let evidenceImportOriginalSha = $state('');
  let backupPassphrase = $state('');
  let backupConfirmation = $state('');
  let backupResultPath = $state('');
  let projectPassphrase = $state('');
  let projectConfirmation = $state('');
  let projectArchivePath = $state('');
  let projectRestorePath = $state('');
  let projectRestorePassphrase = $state('');
  let projectRestoredDirectory = $state('');
  let projectBackupError = $state('');
  let projectRestoreError = $state('');
  let restorePackagePath = $state('');
  let workflowBrief = $state('');
  let workflowStepCount = $state(3);
  let workflowSteps = $state<WorkflowStepDraft[]>([
    { id: 'research', title: 'Research', agentId: '', providerId: '', instruction: 'Gather supporting evidence and identify uncertainties.' },
    { id: 'write', title: 'Write', agentId: '', providerId: '', instruction: 'Draft a clear response using the research findings. Separate evidence from inference.' },
    { id: 'review', title: 'Review', agentId: '', providerId: '', instruction: 'Review the draft for unsupported claims, mistakes, and missing context.' },
  ]);
  let workflowDraftId = $state('');
  let validatedWorkflowJson = $state('');
  let validatedWorkflowSignature = $state('');
  let workflowPreview = $state<WorkflowPreview | null>(null);
  let savedWorkflows = $state<SavedWorkflow[]>([]);
  const workflowPageSize = 25;
  let workflowOffset = $state(0);
  let workflowSearchQuery = $state('');
  let workflowAppliedSearch = $state('');
  let workflowHasMore = $state(false);
  let workflowLoading = $state(false);
  let workflowLoaded = $state(false);
  let workflowError = $state('');
  let workflowNotice = $state('');
  let workflowBusy = $state(false);
  let workflowCancelBusy = $state(false);
  let workflowCancelConfirmation = $state('');
  let displayedWorkflowReportId = $state('');
  let displayedWorkflowMarkdown = $state('');
  let restorePassphrase = $state('');
  let restoredDatabasePath = $state('');
  let backupBusy = $state(false);
  let backupCreateError = $state('');
  let backupRestoreError = $state('');
  let backupOperation = $state<'create' | 'restore' | 'project-create' | 'project-restore' | null>(null);
  let isNative = $state(false);
  let runtimeChecked = $state(false);
  // Bound the number of mounted message nodes even in very large rooms.
  const messagePageSize = 100;
  let messagePage = $state(0);
  let messageQuery = $state('');
  let chatScrollElement = $state<HTMLDivElement | null>(null);
  let chatNearBottom = $state(true);
  let unreadMessageCount = $state(0);

  const room = $derived(snapshot.rooms.find((r) => r.id === roomId));
  const messages = $derived(snapshot.messages.filter((m) => m.room_id === roomId));
  const matchingMessages = $derived(messages.filter((message) => {
    const query = messageQuery.trim().toLocaleLowerCase();
    return !query || `${message.sender_name} ${message.body}`.toLocaleLowerCase().includes(query);
  }));
  const maxMessagePage = $derived(Math.max(0, Math.ceil(matchingMessages.length / messagePageSize) - 1));
  const visibleMessageEnd = $derived(Math.max(0, matchingMessages.length - Math.min(messagePage, maxMessagePage) * messagePageSize));
  const visibleMessages = $derived(matchingMessages.slice(Math.max(0, visibleMessageEnd - messagePageSize), visibleMessageEnd));
  const selectedAgentIds = $derived((selectedAgentsByRoom[roomId] ?? (room?.members_configured ? room.member_ids : snapshot.agents.slice(0, 1).map((agent) => agent.id))).filter((id) => snapshot.agents.some((agent) => agent.id === id)));
  const unsavedRoomTeam = $derived(roomId !== '' && selectedAgentsByRoom[roomId] !== undefined);
  const selectedProvider = $derived(snapshot.providers.find((provider) => provider.id === selectedProviderId));
  const providerPreset = $derived(providerPresets[providerKind]);
  const documentBytes = $derived(utf8Bytes(evidenceText));
  const queryBytes = $derived(utf8Bytes(evidenceQuery.trim()));
  const documentTooLarge = $derived(documentBytes > maxEvidenceDocumentBytes);
  const queryTooLarge = $derived(queryBytes > maxEvidenceQueryBytes);
  const usingAgentProviders = $derived(selectedProviderId === 'each-agent');
  const assignedProvidersReady = $derived(
    selectedAgentIds.length > 0 && selectedAgentIds.every((id) => {
      const agent = snapshot.agents.find((entry) => entry.id === id);
      return !!agent?.provider_id && snapshot.providers.some((provider) => provider.id === agent.provider_id);
    })
  );
  // Retrying an incomplete team request is legitimate even when one agent
  // already replied. The Rust backend skips already-answered agent IDs for the
  // most recent user prompt; don't strand failed teammates behind this gate.
  const canAsk = $derived(isNative && !busy && !replyBusy && !!roomId && (!!selectedProvider || (usingAgentProviders && assignedProvidersReady)) && selectedAgentIds.length > 0 && messages.some((message) => message.sender_kind === 'user'));
  const roomTasks = $derived(snapshot.tasks.filter((task) => !taskFilterRoomId || task.room_id === taskFilterRoomId).slice().reverse());
  const homeRooms = $derived(snapshot.rooms.slice(-5).reverse());
  const homeOpenTasks = $derived(snapshot.tasks.filter((task) => ['pending', 'queued', 'running'].includes(task.state)).slice(-4).reverse());
  const visibleRoomTasks = $derived(roomTasks.filter((task) => {
    const term = taskSearch.trim().toLocaleLowerCase();
    const agent = snapshot.agents.find((item) => item.id === task.agent_id);
    const inSearch = !term || [task.instructions, agent?.name ?? '', agent?.role ?? '']
      .some((item) => item.toLocaleLowerCase().includes(term));
    const inStatus = taskStatusFilter === 'all'
      || (taskStatusFilter === 'open' && ['pending', 'queued', 'running'].includes(task.state))
      || (taskStatusFilter === 'done' && task.state === 'succeeded')
      || (taskStatusFilter === 'attention' && ['failed', 'cancelled'].includes(task.state));
    return inSearch && inStatus;
  }));
  const taskPagination = $derived(paginateLocal(visibleRoomTasks, taskPage, taskPageSize));
  const taskLastPage = $derived(taskPagination.lastPage);
  const currentTaskPage = $derived(taskPagination.page);
  const pagedRoomTasks = $derived(taskPagination.items);
  const taskCounts = $derived({
    total: roomTasks.length,
    awaiting: roomTasks.filter((task) => task.state === 'pending' || task.state === 'queued').length,
    running: roomTasks.filter((task) => task.state === 'running').length,
    succeeded: roomTasks.filter((task) => task.state === 'succeeded').length,
    incomplete: roomTasks.filter((task) => task.state === 'failed' || task.state === 'cancelled').length,
  });
  const activeWorkflowSteps = $derived(workflowSteps.slice(0, workflowStepCount));
  const workflowSignature = $derived(JSON.stringify({
    brief: workflowBrief.trim(), steps: activeWorkflowSteps.map(({ id, agentId, providerId, instruction }) => ({
      id, agentId, providerId, instruction: instruction.trim()
    }))
  }));
  const workflowPreviewCurrent = $derived(!!workflowPreview && !!validatedWorkflowJson && validatedWorkflowSignature === workflowSignature);
  const roleGroups = roleCategories(starterRoles);
  const filteredRoles = $derived(filterRoleLibrary(starterRoles, search, roleCategory));
  const visibleRoles = $derived(filteredRoles.slice(0, rolePageSize));
  const filteredRooms = $derived(filterWorkspaceRooms(snapshot.rooms, roomSearch));
  const filteredProviders = $derived(filterProviderDirectory(snapshot.providers, probeResults, providerSearch, providerStatusFilter));

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
      const previousRoomId = roomId;
      const previousRoomMessageIds = new Set(snapshot.messages.filter((message) => message.room_id === previousRoomId).map((message) => message.id));
      const stayAtBottom = chatNearBottom && messagePage === 0 && !messageQuery.trim();
      const visibleAnchor = !stayAtBottom ? firstVisibleMessageAnchor() : null;
      const next = await invoke<Snapshot>('studio_bootstrap');
      snapshot = { ...next, tasks: next.tasks ?? [] };
      if (!next.rooms.some((r) => r.id === roomId)) selectRoom(next.rooms[0]?.id ?? '');
      if (selectedProviderId && selectedProviderId !== 'each-agent' && !next.providers.some((provider) => provider.id === selectedProviderId)) selectedProviderId = '';
      if (taskFilterRoomId && !next.rooms.some((r) => r.id === taskFilterRoomId)) taskFilterRoomId = '';
      if (screen === 'rooms') {
        await tick();
        if (previousRoomId !== roomId || stayAtBottom) scrollChatToBottom();
        else {
          restoreMessageAnchor(visibleAnchor);
          const added = next.messages.filter((message) => message.room_id === roomId && !previousRoomMessageIds.has(message.id)).length;
          if (added > 0) unreadMessageCount += added;
        }
      }
      error = '';
    } catch (cause) {
      error = `Could not open local workspace: ${String(cause)}`;
    }
  }
  function chatScrollChanged(event: Event) {
    const element = event.currentTarget as HTMLDivElement;
    chatNearBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 96;
    if (chatNearBottom && messagePage === 0 && !messageQuery.trim()) unreadMessageCount = 0;
  }
  function scrollChatToBottom() {
    if (!chatScrollElement) return;
    chatScrollElement.scrollTop = chatScrollElement.scrollHeight;
    chatNearBottom = true;
    unreadMessageCount = 0;
  }
  function firstVisibleMessageAnchor(): { id: string; top: number } | null {
    if (!chatScrollElement) return null;
    const containerTop = chatScrollElement.getBoundingClientRect().top;
    const first = [...chatScrollElement.querySelectorAll<HTMLElement>('[data-message-id]')]
      .find((element) => element.getBoundingClientRect().bottom > containerTop + 8);
    return first ? { id: first.dataset.messageId ?? '', top: first.getBoundingClientRect().top } : null;
  }
  function restoreMessageAnchor(anchor: { id: string; top: number } | null) {
    if (!anchor || !chatScrollElement) return;
    const visible = [...chatScrollElement.querySelectorAll<HTMLElement>('[data-message-id]')]
      .find((element) => element.dataset.messageId === anchor.id);
    if (visible) chatScrollElement.scrollTop += visible.getBoundingClientRect().top - anchor.top;
  }
  async function jumpToLatestMessage() {
    messageQuery = '';
    messagePage = 0;
    await tick();
    scrollChatToBottom();
  }
  async function showMessagePage(nextPage: number) {
    messagePage = Math.max(0, Math.min(maxMessagePage, nextPage));
    await tick();
    chatScrollElement?.querySelector<HTMLElement>('[data-message-id]')?.scrollIntoView({ block: 'start', behavior: 'auto' });
    chatNearBottom = false;
  }
  async function hydrateComposerDrafts() {
    if (!isNative || draftReady) return;
    try {
      const rows = await invoke<StoredDraft[]>('studio_list_room_drafts');
      draftPersistence.hydrate(rows);
      draftsByRoom = Object.fromEntries(rows.map((entry) => [entry.room_id, entry.content]));
      draft = draftsByRoom[roomId] ?? '';
      draftReady = true;
      draftLoadError = '';
    } catch (cause) {
      draftLoadError = `Could not recover saved composer drafts: ${String(cause)}. Retry before writing.`;
    }
  }
  function changeComposerDraft(value: string) {
    if (!draftReady || !roomId) return;
    draft = value;
    draftsByRoom = { ...draftsByRoom, [roomId]: value };
    draftPersistence.change(roomId, value);
  }
  async function restoreSavedDraft() {
    const target = roomId;
    if (!target || busy || replyBusy) return;
    try {
      const rows = await invoke<StoredDraft[]>('studio_list_room_drafts');
      const row = rows.find((item) => item.room_id === target);
      draftPersistence.replaceFromDisk(target, row);
      const saved = row?.content ?? '';
      draftsByRoom = { ...draftsByRoom, [target]: saved };
      if (roomId === target) draft = saved;
      notice = 'Reloaded the draft saved on this device. Local unsaved changes for this room were discarded.';
    } catch (cause) { error = `Cannot reload this draft while a save is active: ${String(cause)}`; }
  }
  function setTheme(mode: 'system' | 'light' | 'dark') {
    themeMode = mode;
    if (mode === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = mode;
    try { window.localStorage.setItem('constellation-appearance', mode); }
    catch { /* User storage can be disabled; keep the session's selection. */ }
  }

  onMount(() => {
    try {
      const saved = window.localStorage.getItem('constellation-appearance');
      if (saved === 'light' || saved === 'dark') setTheme(saved);
    } catch { /* Use OS appearance without browser storage. */ }
    void (async () => { await refresh(); await hydrateComposerDrafts(); })();
    const warnUnsaved = (event: BeforeUnloadEvent) => {
      if (!draftPersistence.hasUnsaved()) return;
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', warnUnsaved);
    return () => {
      window.removeEventListener('beforeunload', warnUnsaved);
      draftPersistence.dispose();
    };
  });

  async function createRoom() {
    if (!newRoom.trim() || !isNative) return;
    working = true; error = '';
    try {
      const created = await invoke<Room>('studio_create_room', { title: newRoom.trim() });
      newRoom = '';
      await refresh(); selectRoom(created.id);
      if (taskSetupReturn === 'new-room') {
        taskRoomId = created.id;
        taskSetupReturn = null;
        dialog = 'new-task';
        await focusCurrentDialogField();
        notice = 'Room created. Continue planning your task.';
      } else { dialog = null; screen = 'rooms'; }
    } catch (cause) { error = String(cause); }
    finally { working = false; }
  }

  async function createAgent(roleName: string, name: string) {
    if (!isNative) return;
    working = true; error = '';
    try {
      const beforeIds = new Set(snapshot.agents.map((agent) => agent.id));
      await invoke('studio_add_agent', { role: roleName, name: name.trim() || roleName });
      await refresh();
      if (taskSetupReturn === 'new-agent') {
        taskAgentId = snapshot.agents.find((agent) => !beforeIds.has(agent.id))?.id ?? taskAgentId;
        taskSetupReturn = null;
        dialog = 'new-task';
        await focusCurrentDialogField();
        notice = 'Agent created. Continue planning your task.';
      } else {
        dialog = null; screen = 'rooms';
        notice = `${name.trim() || roleName} added. Choose a provider before asking this agent to reply.`;
      }
    } catch (cause) { error = String(cause); }
    finally { working = false; }
  }

  async function sendMessage() {
    if (!draft.trim() || !roomId || !isNative || !draftReady || busy || replyBusy) return;
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
      if (!(await draftPersistence.flush(targetRoomId))) {
        throw new Error('Draft could not be saved locally. Resolve the draft error before sending.');
      }
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
        draftPersistence.change(targetRoomId, '', true);
      }
      const draftCleared = await draftPersistence.flush(targetRoomId);
      await refresh();
      // Sending saves locally. External inference is a separate, explicit action.
      replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { providerName: '', replies: [], requestedIds: [] } };
      notice = draftCleared
        ? 'Message saved locally. Choose a provider and use Ask selected or Ask team to request a reply.'
        : 'Message saved, but its composer draft could not be cleared. Review saved text before resending after restart.';
      // The success banner changes available transcript height after refresh.
      // Re-anchor only the room whose message was just saved, after the
      // banner/composer reflow, so the new message is actually visible.
      if (roomId === targetRoomId && !messageQuery.trim() && messagePage === 0) {
        await tick();
        scrollChatToBottom();
      }
    } catch (cause) {
      error = persisted ? `Message was saved, but the room could not refresh: ${String(cause)}` : `Could not confirm the message was saved: ${String(cause)}. Retrying the same text in this session will reuse its operation ID.`;
      if (!persisted && roomId === targetRoomId) draft = content;
    }
    finally { busy = false; }
  }

  function selectRoom(nextId: string) {
    if (nextId !== roomId) {
      if (roomId && draftReady) {
        draftsByRoom = { ...draftsByRoom, [roomId]: draft };
        void draftPersistence.flush(roomId);
      }
      draft = draftsByRoom[nextId] ?? '';
      roomId = nextId;
      messagePage = 0;
      messageQuery = '';
      unreadMessageCount = 0;
      chatNearBottom = true;
      void tick().then(scrollChatToBottom);
      // Explicitly reconfirm the recipient model for each new room. Keeping
      // the previous room's selected provider could export unrelated history.
      selectedProviderId = '';
    }
    sidebarOpen = false;
  }

  function toggleAgent(id: string, checked: boolean) {
    const ids = checked ? [...new Set([...selectedAgentIds, id])] : selectedAgentIds.filter((selected) => selected !== id);
    selectedAgentsByRoom = { ...selectedAgentsByRoom, [roomId]: ids };
  }

  async function saveRoomTeam() {
    if (!isNative || working || replyBusy || busy || !room || !unsavedRoomTeam || selectedAgentIds.length > 8) return;
    const targetRoomId = room.id;
    const memberIds = [...selectedAgentIds];
    working = true; error = '';
    try {
      await invoke('studio_set_room_members', {
        roomId: targetRoomId, memberIds,
        expectedMemberIds: room.members_configured ? room.member_ids : null,
      });
      // Discard the draft only for the room saved. Concurrent room selection
      // or new checkbox input must not be clobbered by a stale IPC response.
      if (JSON.stringify(selectedAgentsByRoom[targetRoomId]) === JSON.stringify(memberIds)) {
        const remaining = { ...selectedAgentsByRoom };
        delete remaining[targetRoomId];
        selectedAgentsByRoom = remaining;
      }
      await refresh();
      notice = 'Room team saved locally. This selection does not grant model or tool permissions.';
    } catch {
      error = 'Could not save room team. Another window may have changed it; reload the room before retrying. No model request was sent.';
    } finally { working = false; }
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

  async function requestTeamReplies(room: string, ids: string[], provider: Provider): Promise<unknown> {
    if (!streamAnswers || provider.kind === 'google_gemini') {
      return invoke<unknown>('studio_generate_team_replies', {
        roomId: room, agentIds: ids, providerId: provider.id
      });
    }
    const onEvent = new Channel<StreamReplyDelta>();
    let acceptingEvents = true;
    onEvent.onmessage = (event) => {
      if (!acceptingEvents || !replyBusy || !streamAnswers || event?.room_id !== room || !ids.includes(event?.agent_id) ||
          typeof event?.delta !== 'string') return;
      if (event.ready === true) {
        streamingRoomId = room;
        return;
      }
      if (event.ready !== false || streamStopRequested) return;
      // Display bounded provisional text only. Native `finish` and the durable
      // TeamReplyBatch determine success, never an arriving text delta.
      const original = streamPreviewByRoom[room]?.[event.agent_id] ?? '';
      const next = (original + event.delta).slice(-4096);
      streamPreviewByRoom = {
        ...streamPreviewByRoom,
        [room]: { ...(streamPreviewByRoom[room] ?? {}), [event.agent_id]: next }
      };
    };
    try {
      return await invoke<unknown>('studio_generate_team_streaming_replies', {
        roomId: room, agentIds: ids, providerId: provider.id, onEvent
      });
    } catch (cause) {
      // A native decoder error makes the provisional text unusable. When
      // distinct providers are contacted sequentially, remove only this
      // failed invocation's drafts: other agents may have confirmed results.
      acceptingEvents = false;
      const surviving = { ...(streamPreviewByRoom[room] ?? {}) };
      for (const agentId of ids) delete surviving[agentId];
      streamPreviewByRoom = { ...streamPreviewByRoom, [room]: surviving };
      throw cause;
    } finally {
      acceptingEvents = false;
      if (streamingRoomId === room) streamingRoomId = '';
    }
  }

  async function stopStreamingReply() {
    if (!isNative || !replyBusy || !streamAnswers || !streamingRoomId || replyCancelBusy) return;
    const target = streamingRoomId;
    replyCancelBusy = true;
    // Stop subsequent per-agent dispatch even if the last native request
    // finished before cancellation was delivered.
    streamStopRequested = true;
    try {
      const result = await invoke<{ room_id: string; cancelled: boolean }>('studio_cancel_room_stream', { roomId: target });
      if (result.room_id !== target || typeof result.cancelled !== 'boolean') {
        throw new Error('Invalid cancellation confirmation');
      }
      if (!result.cancelled) {
        notice = 'No active model stream remained to interrupt. Earlier completed replies may be saved.';
      }
    } catch {
      error = 'Could not confirm stream interruption. Check the latest results before asking again.';
    } finally {
      replyCancelBusy = false;
    }
  }

  async function askAgents(mode: 'selected' | 'team') {
    if (!canAsk) return;
    const targetRoomId = roomId;
    const provider = selectedProvider;
    const routeByAgent = usingAgentProviders;
    const providerName = routeByAgent ? 'Each agent’s assigned model' : provider?.name ?? 'Unknown provider';
    const ids = mode === 'selected' ? selectedAgentIds.slice(0, 1) : [...selectedAgentIds];
    replyBusy = true;
    streamStopRequested = false;
    streamingRoomId = '';
    error = '';
    streamPreviewByRoom = { ...streamPreviewByRoom, [targetRoomId]: {} };
    replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { requestedIds: ids, providerName, replies: [] } };
    try {
      if (routeByAgent) {
        // Capture route decisions once when the user explicitly presses Ask.
        // A team can therefore use different providers without a silent
        // default or sending any message to a model the user didn't select.
        const routes = ids.map((id) => ({
          id, providerId: snapshot.agents.find((agent) => agent.id === id)?.provider_id
        }));
        const replies: TeamReply[] = [];
        for (const route of routes) {
          if (streamStopRequested) {
            replies.push({ agent_id: route.id, status: 'skipped', error: 'Stopped before this model was contacted' });
            continue;
          }
          if (!route.providerId || !snapshot.providers.some((item) => item.id === route.providerId)) {
            replies.push({ agent_id: route.id, status: 'failed', error: 'No saved provider for this agent' });
          } else {
            try {
              const configured = snapshot.providers.find((item) => item.id === route.providerId)!;
              const raw = await requestTeamReplies(targetRoomId, [route.id], configured);
              const confirmed = parseTeamReplies(raw, targetRoomId, route.providerId, [route.id]);
              replies.push(confirmed[0] ?? {
                agent_id: route.id, status: 'failed', error: 'Provider returned no confirmed result'
              });
            } catch (cause) {
              replies.push({
                agent_id: route.id, status: 'failed',
                error: `No confirmed result: ${String(cause)}`
              });
            }
          }
          replyFeedbackByRoom = {
            ...replyFeedbackByRoom,
            [targetRoomId]: { requestedIds: ids, providerName, replies: [...replies] }
          };
        }
      } else {
        if (!provider) throw new Error('Select a provider before asking.');
        const raw = await requestTeamReplies(targetRoomId, ids, provider);
        const replies = parseTeamReplies(raw, targetRoomId, provider.id, ids);
        replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { requestedIds: ids, providerName, replies } };
      }
      await refresh();
    } catch (cause) {
      // A request can have partially succeeded before it failed. Retain that
      // uncertainty and refresh persisted messages without replaying the call.
      replyFeedbackByRoom = { ...replyFeedbackByRoom, [targetRoomId]: { requestedIds: ids, providerName, replies: replyFeedbackByRoom[targetRoomId]?.replies ?? [], error: `Request failed or returned no usable status: ${String(cause)}. Check the room for saved replies before trying again.` } };
      try { await refresh(); } catch { /* keep original failure */ }
    } finally {
      replyBusy = false;
      streamStopRequested = false;
      streamingRoomId = '';
      streamPreviewByRoom = { ...streamPreviewByRoom, [targetRoomId]: {} };
    }
  }

  async function assignAgentProvider(agentId: string, providerId: string) {
    if (!isNative || working || replyBusy) return;
    working = true; error = '';
    try {
      await invoke('studio_set_agent_provider', {
        agentId, providerId: providerId || null
      });
      await refresh();
      notice = providerId ? 'Agent model preference saved locally. No model request was sent.' : 'Agent model preference cleared.';
    } catch (cause) {
      error = `Could not save agent model preference: ${String(cause)}`;
    } finally {
      working = false;
    }
  }

  function chooseProviderKind(nextKind: ProviderKind) {
    if (nextKind === providerKind) return;
    // Never carry a credential from one transport/host template to another.
    const preset = providerPresets[nextKind];
    providerKind = nextKind;
    providerName = preset.name;
    providerEndpoint = preset.endpoint;
    providerModel = '';
    providerKey = '';
    providerRememberKey = false;
  }

  function openProviderDialog() {
    // "Add provider" starts a fresh compatible profile rather than silently
    // cloning the previous host/model (or leaking its session credential).
    providerKind = 'custom_compatible';
    providerName = providerPresets.custom_compatible.name;
    providerEndpoint = providerPresets.custom_compatible.endpoint;
    providerModel = '';
    providerKey = '';
    providerRememberKey = false;
    providerRevisionSourceId = '';
    providerRevisionSavedKey = false;
    dialog = 'provider';
  }

  function openProviderRevision(profile: Provider) {
    const template = prepareProviderRevision(profile);
    // The dialog never retrieves, copies or reveals any existing API secret.
    if (!Object.prototype.hasOwnProperty.call(providerPresets, template.kind)) {
      error = 'This legacy provider format cannot be revised with the current form.';
      return;
    }
    providerRevisionSourceId = template.sourceId;
    providerRevisionSavedKey = template.sourceRememberedKey;
    providerName = template.name;
    providerKind = template.kind as ProviderKind;
    providerEndpoint = template.endpoint;
    providerModel = template.model;
    providerKey = template.key;
    providerRememberKey = template.rememberKey;
    dialog = 'provider';
  }

  function closeDialog() {
    if (dialog === 'provider') {
      providerKey = '';
      providerRememberKey = false;
      providerRevisionSourceId = '';
      providerRevisionSavedKey = false;
    }
    dialog = null;
    taskSetupReturn = null;
  }
  async function focusCurrentDialogField() {
    await tick();
    document.querySelector<HTMLElement>('.modal input:not([disabled]), .modal select:not([disabled])')?.focus();
  }

  async function copyMessageText(content: string) {
    try {
      await navigator.clipboard.writeText(content);
      notice = 'Message text copied. Nothing was sent to an AI provider.';
    } catch (cause) {
      error = `Could not copy message text: ${String(cause)}`;
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

  function clearEvidenceResults() {
    // Results describe an exact source snapshot and query. Never show stale
    // quotations once either field changes.
    evidenceHits = [];
    evidenceSearched = false;
    evidenceError = '';
    evidenceSavedResults = false;
    evidenceSavedOffset = 0;
    evidenceSavedHasMore = false;
    evidenceNotice = '';
  }

  function forgetImportedProvenance() {
    // Original-file page coordinates belong to the exact extracted-text
    // snapshot. Any manual text modification invalidates them immediately.
    evidenceImportAnchors = [];
    evidenceImportSourceSha = '';
    evidenceImportOriginalSha = '';
    evidenceImportInfo = '';
    clearEvidenceResults();
  }

  function evidenceAnchorsFor(hit: EvidenceHit): EvidenceImportAnchor[] {
    if (evidenceSavedResults || !evidenceImportSourceSha ||
        hit.source_sha256 !== evidenceImportSourceSha) return [];
    return evidenceImportAnchors.filter((anchor) =>
      anchor.start_byte < hit.end_byte && anchor.end_byte > hit.start_byte
    );
  }

  function evidenceAnchorLabel(hit: EvidenceHit): string {
    return evidenceAnchorsFor(hit).map((anchor) =>
      `${anchor.kind === 'pdf_page' ? 'PDF page' : 'DOCX paragraph'} ${anchor.number}`
    ).join(', ');
  }

  async function refreshSavedEvidence() {
    if (!isNative) return;
    evidenceInventoryError = '';
    try {
      const rows = await invoke<SavedEvidence[]>('studio_list_saved_evidence');
      if (!Array.isArray(rows) || rows.length > 128 || rows.some((source) =>
        typeof source.source_id !== 'string' || typeof source.title !== 'string' ||
        typeof source.relative_path !== 'string' || !/^[0-9a-f]{64}$/i.test(source.source_sha256) ||
        !Number.isSafeInteger(source.bytes) || source.bytes < 1 || source.bytes > maxEvidenceDocumentBytes
      )) throw new Error('Unrecognized local source inventory');
      evidenceSavedSources = rows;
      evidenceInventoryLoaded = true;
      if (evidenceDeleteConfirmation && !rows.some((row) => row.source_id === evidenceDeleteConfirmation?.sourceId && row.source_sha256 === evidenceDeleteConfirmation?.sha256)) {
        evidenceDeleteConfirmation = null;
      }
      if (evidenceLoadConfirmation && !rows.some((row) => row.source_id === evidenceLoadConfirmation?.sourceId && row.source_sha256 === evidenceLoadConfirmation?.sha256)) {
        evidenceLoadConfirmation = null;
      }
    } catch {
      evidenceSavedSources = [];
      evidenceInventoryLoaded = false;
      evidenceInventoryError = 'Saved sources could not be verified. Refresh the library before loading or removing a source.';
    }
  }

  async function deleteSavedEvidence(source: SavedEvidence) {
    if (!isNative || evidenceBusy || evidenceSaving) return;
    if (evidenceDeleteConfirmation?.sourceId !== source.source_id ||
        evidenceDeleteConfirmation?.sha256 !== source.source_sha256) {
      evidenceLoadConfirmation = null;
      evidenceDeleteConfirmation = {sourceId: source.source_id, sha256: source.source_sha256};
      return;
    }
    evidenceSaving = true;
    evidenceError = '';
    try {
      const response = await invoke<{deleted: boolean; source_id: string; model_called: boolean}>(
        'studio_delete_saved_evidence', {sourceId: source.source_id, expectedSha256: source.source_sha256}
      );
      if (response.deleted !== true || response.source_id !== source.source_id || response.model_called !== false) {
        throw new Error('Deletion was not verified');
      }
      evidenceDeleteConfirmation = null;
      evidenceLoadConfirmation = null;
      clearEvidenceResults();
      await refreshSavedEvidence();
      evidenceNotice = 'Stored source removed from this project. The current draft was not cleared; earlier encrypted backups may still include it.';
      await tick();
      document.getElementById('evidence-saved-section')?.focus();
    } catch {
      evidenceError = 'The saved source could not be removed. Its contents may have changed; refresh and review again.';
      evidenceDeleteConfirmation = null;
    } finally {
      evidenceSaving = false;
    }
  }

  async function importEvidenceDocument(file: File | undefined, replaceDraft = false) {
    if (!file || !isNative || !evidenceImportRights || evidenceBusy || evidenceImportBusy) return;
    if (evidenceText.trim() && !replaceDraft) {
      evidencePendingImport = file;
      evidenceError = '';
      return;
    }
    evidencePendingImport = null;
    evidenceBusy = true;
    evidenceImportBusy = true;
    evidenceError = '';
    evidenceNotice = '';
    evidenceImportInfo = '';
    try {
      if (!/\.(pdf|docx)$/i.test(file.name) || utf8Bytes(file.name) > 120 ||
          file.size < 1 || file.size > 2 * 1024 * 1024) {
        throw new Error('Invalid document size or extension');
      }
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const response = await invoke<{
        filename: string; extracted_text: string; original_sha256: string;
        extracted_sha256: string; locations: {start_byte: number; end_byte: number; kind: string; number: number}[];
        model_called: boolean; stored: boolean; original_binary_stored: boolean;
      }>('studio_extract_local_document', {
        fileName: file.name,
        documentBytes: bytes,
        rightsAcknowledged: true,
      });
      if (response.model_called !== false || response.stored !== false || response.original_binary_stored !== false ||
          response.filename !== file.name || typeof response.extracted_text !== 'string' ||
          utf8Bytes(response.extracted_text) > maxEvidenceDocumentBytes ||
          !/^[0-9a-f]{64}$/i.test(response.original_sha256) ||
          !/^[0-9a-f]{64}$/i.test(response.extracted_sha256) ||
          !Array.isArray(response.locations) || response.locations.length > 16_384) {
        throw new Error('Importer returned an invalid source response');
      }
      const outputBytes = utf8Bytes(response.extracted_text);
      if (response.locations.some((location) => !Number.isSafeInteger(location.start_byte) ||
        !Number.isSafeInteger(location.end_byte) || location.start_byte < 0 ||
        location.end_byte > outputBytes || location.start_byte >= location.end_byte ||
        !Number.isSafeInteger(location.number) || location.number < 1 ||
        !['pdf_page', 'docx_paragraph'].includes(location.kind))) {
        throw new Error('Importer returned invalid page/paragraph coordinates');
      }
      evidenceText = response.extracted_text;
      evidenceSaveTitle = file.name.replace(/\.(pdf|docx)$/i, '').slice(0, 90);
      clearEvidenceResults();
      evidenceImportAnchors = response.locations as EvidenceImportAnchor[];
      evidenceImportSourceSha = response.extracted_sha256;
      evidenceImportOriginalSha = response.original_sha256;
      evidenceImportInfo = `Extracted ${outputBytes.toLocaleString()} text bytes from ${response.locations.length} page/paragraph sections. Original file SHA-256: ${response.original_sha256}. The original PDF/DOCX is not stored; Save source is a separate explicit action and saves the extracted text only.`;
      evidenceLoadConfirmation = null;
      await tick();
      document.getElementById('evidence-document')?.focus();
    } catch {
      evidenceError = 'Could not safely extract this PDF/DOCX. Compressed or encrypted PDFs and some document features are not supported. The original file was not saved.';
    } finally {
      evidenceBusy = false;
      evidenceImportBusy = false;
    }
  }

  async function saveEvidence() {
    if (!isNative || evidenceBusy || evidenceSaving || !evidenceSaveTitle.trim() ||
      !evidenceText.trim() || documentTooLarge || utf8Bytes(evidenceSaveTitle.trim()) > 120) return;
    evidenceSaving = true;
    evidenceError = '';
    evidenceNotice = '';
    try {
      await invoke('studio_save_evidence', { title: evidenceSaveTitle.trim(), sourceText: evidenceText });
      await refreshSavedEvidence();
      evidenceNotice = evidenceInventoryError
        ? 'The save request returned, but the library could not be refreshed. Check saved sources before saving again.'
        : 'Source saved locally. It can now be searched with other saved sources. This did not contact an AI provider.';
    } catch {
      evidenceError = 'The source could not be saved. Check its title, text and project limits.';
    } finally {
      evidenceSaving = false;
    }
  }

  async function loadSavedEvidence(source: SavedEvidence) {
    if (!isNative || evidenceBusy || evidenceSaving) return;
    if (evidenceText.trim() && (evidenceLoadConfirmation?.sourceId !== source.source_id ||
        evidenceLoadConfirmation.sha256 !== source.source_sha256)) {
      evidenceLoadConfirmation = {sourceId: source.source_id, sha256: source.source_sha256};
      evidenceDeleteConfirmation = null;
      return;
    }
    evidenceBusy = true;
    evidenceError = '';
    evidenceNotice = '';
    try {
      const loaded = await invoke<SavedEvidenceDetail>('studio_load_saved_evidence', { sourceId: source.source_id });
      if (loaded.source_id !== source.source_id || loaded.source_sha256 !== source.source_sha256 ||
        loaded.relative_path !== source.relative_path || loaded.title !== source.title ||
        loaded.format !== source.format || typeof loaded.text !== 'string' ||
        utf8Bytes(loaded.text) !== loaded.bytes || loaded.bytes !== source.bytes ||
        loaded.bytes > maxEvidenceDocumentBytes) {
        throw new Error('Invalid saved source');
      }
      evidenceText = loaded.text;
      evidenceSaveTitle = loaded.title;
      forgetImportedProvenance();
      evidenceLoadConfirmation = null;
      evidenceNotice = `Loaded “${loaded.title}” into the draft. It is a saved extracted-text snapshot; original PDF/DOCX page positions are not restored.`;
      await tick();
      document.getElementById('evidence-document')?.focus();
    } catch {
      evidenceError = 'The saved source could not be loaded or verified.';
      evidenceLoadConfirmation = null;
    } finally {
      evidenceBusy = false;
    }
  }

  async function searchSavedEvidence(offset = 0) {
    const query = evidenceQuery.trim();
    if (!isNative || evidenceBusy || !query || queryTooLarge ||
        !Number.isSafeInteger(offset) || offset < 0 || offset > 990 || offset % 10 !== 0) return;
    evidenceBusy = true;
    evidenceError = '';
    evidenceHits = [];
    evidenceSearched = false;
    try {
      const response = await invoke<{ results: EvidenceHit[]; model_called: boolean; stored: boolean; offset: number; page_size: number; max_ranked_hits: number; has_more: boolean }>(
        'studio_search_saved_evidence', { query, offset }
      );
      if (response.model_called !== false || response.stored !== true ||
        response.offset !== offset || response.page_size !== 10 || response.max_ranked_hits !== 1_000 ||
        typeof response.has_more !== 'boolean' ||
        !Array.isArray(response.results) || response.results.length > 10 ||
        (response.has_more && response.results.length !== 10)) throw new Error('Invalid saved search');
      // Independently check each quoted byte range against the exact saved
      // source fetched through trusted Rust. If it changes, discard results.
      const sourceIds = [...new Set(response.results.map((hit) => hit.source_id))];
      const sources = await Promise.all(sourceIds.map((sourceId) =>
        invoke<SavedEvidenceDetail>('studio_load_saved_evidence', { sourceId })
      ));
      const bytesById = new Map(sources.map((source) => [source.source_id, new TextEncoder().encode(source.text)]));
      const decoder = new TextDecoder('utf-8', { fatal: true });
      for (const hit of response.results) {
        const bytes = bytesById.get(hit.source_id);
        const source = sources.find((item) => item.source_id === hit.source_id);
        if (!bytes || !source || source.source_sha256 !== hit.source_sha256 ||
          hit.relative_path !== source.relative_path ||
          !Number.isSafeInteger(hit.start_byte) || !Number.isSafeInteger(hit.end_byte) ||
          hit.start_byte < 0 || hit.end_byte <= hit.start_byte || hit.end_byte > bytes.length ||
          !Number.isFinite(hit.bm25_score) || decoder.decode(bytes.slice(hit.start_byte, hit.end_byte)) !== hit.quote) {
          throw new Error('Saved citation failed original-source verification');
        }
      }
      if (evidenceQuery.trim() !== query) return;
      evidenceHits = response.results;
      evidenceSearched = true;
      evidenceSavedResults = true;
      evidenceSavedOffset = response.offset;
      evidenceSavedHasMore = response.has_more;
      await refreshSavedEvidence();
    } catch {
      if (evidenceQuery.trim() === query) evidenceError = 'Saved-source search could not be verified.';
    } finally {
      evidenceBusy = false;
    }
  }

  async function copySavedEvidencePack() {
    const query = evidenceQuery.trim();
    if (!isNative || evidenceBusy || evidenceSaving || !query || queryTooLarge) return;
    evidenceBusy = true;
    evidenceError = '';
    try {
      const markdown = await invoke<string>('studio_export_saved_evidence_markdown', {query});
      if (evidenceQuery.trim() !== query || typeof markdown !== 'string' ||
        !markdown.startsWith('# Constellation saved evidence excerpts\n') ||
        utf8Bytes(markdown) > 150_000) return;
      // Explicit copy places local research text on the operating system
      // clipboard. No upload or provider request is made by Constellation.
      await navigator.clipboard.writeText(markdown);
      notice = 'Saved-source excerpts copied with exact hashes and byte coordinates. Clipboard contents may be visible to other applications.';
    } catch {
      evidenceError = 'Could not copy the verified saved-source excerpts.';
    } finally {
      evidenceBusy = false;
    }
  }

  async function searchEvidence() {
    if (!isNative || evidenceBusy) return;
    const sourceText = evidenceText;
    const query = evidenceQuery.trim();
    evidenceHits = [];
    evidenceSearched = false;
    evidenceSavedResults = false;
    evidenceSavedOffset = 0;
    evidenceSavedHasMore = false;
    evidenceError = '';
    if (!sourceText.trim() || !query) {
      evidenceError = 'Add a document and search terms.';
      return;
    }
    if (utf8Bytes(sourceText) > maxEvidenceDocumentBytes || utf8Bytes(query) > maxEvidenceQueryBytes) {
      evidenceError = 'The document must be at most 128 KiB and the query at most 2048 bytes.';
      return;
    }
    evidenceBusy = true;
    try {
      const response = await invoke<{ results: EvidenceHit[]; model_called: boolean; stored: boolean }>(
        'studio_search_evidence', { sourceText, query }
      );
      // The desktop backend is authoritative for evidence. Reject malformed
      // offsets and anything that does not match the user's actual source.
      const bytes = new TextEncoder().encode(sourceText);
      const decoder = new TextDecoder('utf-8', { fatal: true });
      if (response.model_called !== false || response.stored !== false || !Array.isArray(response.results) || response.results.length > 5) {
        throw new Error('The evidence response did not match the local search contract.');
      }
      const validated = response.results.map((hit) => {
        if (!Number.isSafeInteger(hit.start_byte) || !Number.isSafeInteger(hit.end_byte) ||
          hit.start_byte < 0 || hit.end_byte <= hit.start_byte || hit.end_byte > bytes.length ||
          hit.source_id !== 'pasted-document' || hit.relative_path !== 'pasted-document.md' ||
          typeof hit.source_sha256 !== 'string' || !/^[0-9a-f]{64}$/i.test(hit.source_sha256) ||
          typeof hit.quote !== 'string' || !Number.isFinite(hit.bm25_score) ||
          decoder.decode(bytes.slice(hit.start_byte, hit.end_byte)) !== hit.quote) {
          throw new Error('A search result could not be matched to the source.');
        }
        return hit;
      });
      if (evidenceText !== sourceText || evidenceQuery.trim() !== query) return;
      evidenceHits = validated;
      evidenceSearched = true;
    } catch {
      if (evidenceText === sourceText && evidenceQuery.trim() === query) {
        evidenceError = 'Local document search could not be completed. Check the input and try again.';
      }
    } finally {
      evidenceBusy = false;
    }
  }

  async function copyEvidence(hit: EvidenceHit) {
    if (!evidenceHits.includes(hit)) return;
    try {
      const quote = hit.quote.split('\n').map((line) => `> ${line}`).join('\n');
      const original = evidenceAnchorLabel(hit)
        ? `\nExtracted source: ${evidenceAnchorLabel(hit)} · Original file SHA-256: ${evidenceImportOriginalSha} (verify with the original document)`
        : '';
      await navigator.clipboard.writeText(`${quote}\n\nSource: ${hit.relative_path} · SHA-256: ${hit.source_sha256} · UTF-8 byte range [${hit.start_byte}, ${hit.end_byte})${original}`);
      notice = 'Verbatim quotation and its source coordinates copied.';
    } catch {
      evidenceError = 'Could not copy the quotation to the clipboard.';
    }
  }

  function clearFileToolDraft() {
    fileToolPreview = null;
    fileToolResult = null;
    fileToolError = '';
    fileProposalText = '';
    fileToolOriginProviderId = '';
    finalFollowupError = '';
  }

  async function toggleFileTool(taskId: string) {
    if (fileToolBusy) return;
    if (fileToolPreview) {
      await rejectFileTool();
      if (fileToolPreview) return;
    }
    fileToolTaskId = fileToolTaskId === taskId ? '' : taskId;
    clearFileToolDraft();
    fileToolPath = '';
    fileToolContent = '';
    const task = snapshot.tasks.find((item) => item.id === fileToolTaskId);
    const providerId = task ? taskProviderId(task) : '';
    if (task && providerId) await refreshFileProposalUsage(task.id, providerId);
    else fileProposalUsage = null;
  }

  async function refreshFileProposalUsage(taskId: string, providerId: string) {
    if (!isNative || !providerId) { fileProposalUsage = null; return; }
    try {
      const usage = await invoke<ProposalUsage>('studio_file_proposal_usage', { taskId, providerId });
      if (fileToolTaskId === taskId && Number.isInteger(usage.task_used) && Number.isInteger(usage.provider_used) &&
          usage.task_limit === 16 && usage.provider_limit === 128 &&
          usage.task_used >= 0 && usage.task_used <= usage.task_limit &&
          usage.provider_used >= 0 && usage.provider_used <= usage.provider_limit) {
        fileProposalUsage = { ...usage, taskId, providerId };
      }
    } catch {
      if (fileToolTaskId === taskId) fileProposalUsage = null;
    }
  }

  async function inspectModelRecovery(task: StudioTask) {
    if (!isNative || recoveryInspectionTaskId || task.state !== 'pending') return;
    const requestSequence = ++recoveryInspectionSequence;
    recoveryInspectionTaskId = task.id;
    // Never show an earlier observation as though it were the current
    // immutable-journal query while a new read is in flight.
    const previous = { ...recoveryInspections };
    delete previous[task.id];
    recoveryInspections = previous;
    const errors = { ...recoveryInspectionErrors };
    delete errors[task.id];
    recoveryInspectionErrors = errors;
    try {
      const response = await invoke<unknown>('studio_inspect_model_operation_recovery', { taskId: task.id });
      // A canceled/deleted task or a superseded inspection must never receive
      // an apparently live recovery status from a late native response.
      if (requestSequence !== recoveryInspectionSequence ||
          !snapshot.tasks.some((item) => item.id === task.id && item.state === 'pending')) return;
      recoveryInspections = { ...recoveryInspections, [task.id]: validateModelRecovery(response, task.id) };
    } catch {
      if (requestSequence === recoveryInspectionSequence &&
          snapshot.tasks.some((item) => item.id === task.id && item.state === 'pending')) {
        recoveryInspectionErrors = { ...recoveryInspectionErrors, [task.id]: 'Recovery evidence could not be authenticated or the saved task has no resumable audit. No approval or action was restored. You may inspect the audit again.' };
      }
    } finally {
      if (requestSequence === recoveryInspectionSequence) recoveryInspectionTaskId = '';
    }
  }

  function validateModelRecovery(value: unknown, taskId: string): ModelRecovery {
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid recovery evidence');
    const record = value as Record<string, unknown>;
    if (record.task_id !== taskId || record.automatic_resume_allowed !== false ||
        record.native_approval_restored !== false || !Array.isArray(record.operations) || record.operations.length > 8) {
      throw new Error('Recovery origin or limits mismatch');
    }
    const seen = new Set<string>();
    const statuses: ModelRecoveryStatus[] = ['not_in_journal', 'approved_not_started', 'denied', 'uncertain', 'completed'];
    const outcomes: ModelRecoveryOutcome[] = ['succeeded', 'failed', 'cancelled', 'timed_out', 'output_limit'];
    const operations: ModelRecoveryOperation[] = record.operations.map((raw: unknown) => {
      if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('Invalid operation evidence');
      const operation = raw as Record<string, unknown>;
      if (typeof operation.operation_id !== 'string' || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(operation.operation_id) ||
          seen.has(operation.operation_id) || !Number.isSafeInteger(operation.checkpoint_revision) ||
          (operation.checkpoint_revision as number) < 1 || (operation.checkpoint_revision as number) > 64 ||
          !statuses.includes(operation.status as ModelRecoveryStatus) ||
          (operation.status === 'completed' ? !outcomes.includes(operation.outcome as ModelRecoveryOutcome) : operation.outcome !== null)) {
        throw new Error('Invalid operation chronology or result');
      }
      seen.add(operation.operation_id);
      return operation as ModelRecoveryOperation;
    });
    if (record.original_provider_id !== null &&
        (typeof record.original_provider_id !== 'string' ||
          !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(record.original_provider_id))) {
      throw new Error('Invalid original provider provenance');
    }
    return { task_id: taskId, original_provider_id: record.original_provider_id as string | null,
      automatic_resume_allowed: false, native_approval_restored: false, operations };
  }

  function recoveryStatusLabel(status: ModelRecoveryStatus): string {
    return ({ not_in_journal: 'No authenticated journal entry', approved_not_started: 'Approval recorded; execution not started', denied: 'Permission denied', uncertain: 'Execution outcome uncertain', completed: 'Terminal result recorded' })[status];
  }

  function recoveryStatusExplanation(status: ModelRecoveryStatus): string {
    return ({
      not_in_journal: 'The available journal does not establish whether this action occurred. Do not retry it automatically.',
      approved_not_started: 'A former native approval was recorded, but it cannot be reused. A new action would require a new independent review.',
      denied: 'The action was denied and was not authorized to execute.',
      uncertain: 'The operation may have changed files before interruption. Inspect the private artifacts manually before deciding what to do.',
      completed: 'The authenticated journal has a terminal outcome; this is not permission for another operation.'
    })[status];
  }

  async function prepareFileTool() {
    if (!isNative || fileToolBusy || !fileToolTaskId || fileToolPreview) return;
    fileToolError = '';
    fileToolResult = null;
    fileToolOriginProviderId = '';
    finalFollowupError = '';
    if (!fileToolPath.trim() || utf8Bytes(fileToolPath.trim()) > 1024 ||
      (fileToolAction === 'create_new' && (!fileToolContent || utf8Bytes(fileToolContent) > 16 * 1024))) {
      fileToolError = 'Enter a relative path and, for creation, 1–16384 UTF-8 bytes of text.';
      return;
    }
    const requestedTaskId = fileToolTaskId;
    fileToolBusy = true;
    try {
      const preview = await invoke<FileToolPreview>('studio_prepare_local_file_operation', {
        taskId: requestedTaskId,
        action: fileToolAction,
        relativePath: fileToolPath.trim(),
        content: fileToolAction === 'create_new' ? fileToolContent : null
      });
      if (preview.task_id !== requestedTaskId || preview.action !== fileToolAction ||
        preview.permission_granted !== false || !/^[0-9a-f]{64}$/i.test(preview.digest) ||
        !preview.operation_id || !preview.canonical_path) {
        throw new Error('Invalid immutable preview');
      }
      if (preview.action === 'read' && ('sha256' in preview || 'byte_count' in preview ||
        'reviewed_content' in preview)) {
        // A broken backend must not be allowed to put private read metadata in
        // this renderer. Consume the pending operation before reporting error.
        await invoke('studio_reject_local_file_operation', { operationId: preview.operation_id });
        throw new Error('Read metadata was exposed before native consent');
      }
      if (fileToolTaskId !== requestedTaskId) {
        await invoke('studio_reject_local_file_operation', { operationId: preview.operation_id });
        return;
      }
      fileToolPreview = preview;
    } catch {
      fileToolError = 'Could not prepare the local action. Confirm that the task is pending, the path is inside the private artifacts folder, and the OS credential vault is available.';
    } finally {
      fileToolBusy = false;
    }
  }

  async function requestModelFileProposal(task: StudioTask) {
    const providerId = taskProviderId(task);
    if (!isNative || fileToolBusy || fileToolPreview || working || runningTaskId || !providerId || task.state !== 'pending') return;
    fileToolTaskId = task.id;
    fileToolBusy = true;
    fileToolResult = null;
    fileProposalText = '';
    fileToolOriginProviderId = '';
    finalFollowupError = '';
    fileToolError = '';
    try {
      const suggestion = await invoke<FileToolPreview | { status: 'final_text'; text: string; tools_executed: false; task: StudioTask; result_message_id: string; provider_id: string }>(
        'studio_propose_local_file_operation', { taskId: task.id, providerId }
      );
      if ('status' in suggestion) {
        if (suggestion.status !== 'final_text' || suggestion.tools_executed !== false || !suggestion.text ||
            suggestion.task?.id !== task.id || suggestion.task?.state !== 'succeeded' ||
            !suggestion.result_message_id || suggestion.provider_id !== providerId) {
          throw new Error('Invalid final response');
        }
        fileProposalText = suggestion.text;
        notice = 'The model completed this task without requesting a native file action. Its answer is saved in the room.';
        await refresh();
        return;
      }
      if (suggestion.origin !== 'model_proposal' || suggestion.task_id !== task.id ||
          !['read', 'create_new'].includes(suggestion.action) || suggestion.permission_granted !== false ||
          !/^[0-9a-f]{64}$/i.test(suggestion.digest) || !suggestion.operation_id || !suggestion.canonical_path ||
          (suggestion.action === 'read' && ('sha256' in suggestion || 'byte_count' in suggestion || 'reviewed_content' in suggestion))) {
        if (suggestion.operation_id) {
          await invoke('studio_reject_local_file_operation', { operationId: suggestion.operation_id });
        }
        throw new Error('Invalid or private read preview');
      }
      fileToolAction = suggestion.action;
      fileToolPath = '';
      fileToolContent = '';
      fileToolPreview = suggestion;
      fileToolOriginProviderId = providerId;
      notice = 'The model proposed one file action. Nothing has executed. Review the exact bytes and decide in the native confirmation.';
    } catch (cause) {
      fileToolError = `Could not prepare a model file proposal: ${String(cause)}`;
    } finally {
      fileToolBusy = false;
      void refreshFileProposalUsage(task.id, providerId);
    }
  }

  async function rejectFileTool() {
    if (!fileToolPreview || fileToolBusy) return;
    fileToolBusy = true;
    fileToolError = '';
    try {
      const receipt = await invoke<{ operation_id: string; status: string; executed: boolean }>(
        'studio_reject_local_file_operation', { operationId: fileToolPreview.operation_id }
      );
      if (receipt.status !== 'rejected' || receipt.executed !== false) throw new Error('Invalid denial receipt');
      fileToolPreview = null;
      notice = 'The file action was rejected. No file operation was executed.';
    } catch {
      fileToolError = 'Could not confirm rejection. The operation has not been approved; restart the app to discard any pending action.';
    } finally {
      fileToolBusy = false;
    }
  }

  async function approveFileTool() {
    if (!fileToolPreview || !isNative || fileToolBusy) return;
    const approved = fileToolPreview;
    fileToolBusy = true;
    fileToolError = '';
    try {
      const result = await invoke<FileToolResult | FileToolDenial>('studio_approve_local_file_operation', {
        operationId: approved.operation_id, digest: approved.digest
      });
      if (result.status === 'rejected') {
        if (result.operation_id !== approved.operation_id || result.executed !== false) {
          throw new Error('Invalid native denial receipt');
        }
        fileToolPreview = null;
        fileToolResult = null;
        notice = result.reason === 'User declined the independent native confirmation'
          ? 'You declined the native confirmation. No file action was executed.'
          : 'Native confirmation was unavailable. The file operation was rejected without execution.';
        return;
      }
      if (result.operation_id !== approved.operation_id || result.executed !== true ||
        !['succeeded', 'failed'].includes(result.status) || !Number.isSafeInteger(result.bytes) ||
        (approved.origin === 'model_proposal' && (result.automatic_resume_allowed !== false || typeof result.model_observation_authenticated !== 'boolean')) ||
        (approved.origin !== 'model_proposal' && result.model_observation_authenticated === true)) {
        throw new Error('Unexpected execution receipt');
      }
      fileToolResult = result;
      fileToolPreview = null;
      notice = result.status === 'succeeded' ? 'The approved file action completed locally and an audit receipt was saved.' :
        'The file action failed. Check the result and audit record before any new attempt.';
    } catch {
      // The native host consumes approved IDs before dispatch. Never retry an
      // uncertain write using the same preview, or claim it did not run.
      fileToolPreview = null;
      fileToolError = 'Outcome uncertain or approval rejected. Inspect the file before creating a new action; this one-use request cannot be retried.';
    } finally {
      fileToolBusy = false;
    }
  }

  async function continueAfterReviewedTool(task: StudioTask) {
    const providerId = fileToolOriginProviderId;
    if (!isNative || fileToolBusy || runningTaskId || working || task.state !== 'pending' ||
        fileToolTaskId !== task.id || fileToolResult?.model_observation_authenticated !== true ||
        !snapshot.providers.some((provider) => provider.id === providerId)) return;
    await requestAuthenticatedFinalFollowup(task, providerId);
  }

  async function continueAfterRecoveredTool(task: StudioTask) {
    // The read-only recovery display is not an approval or authorization.
    // All task, provider, exact signed source, journal and revision checks are
    // repeated by the native command before it may reserve another attempt.
    const providerId = recoveryInspections[task.id]?.original_provider_id;
    const verifiedFinished = recoveryInspections[task.id]?.operations.some(
      (item) => item.status === 'completed' && item.outcome !== 'cancelled'
    );
    if (!isNative || !verifiedFinished || !providerId || !snapshot.providers.some((provider) => provider.id === providerId) ||
        fileToolBusy || runningTaskId || working || task.state !== 'pending') return;
    await requestAuthenticatedFinalFollowup(task, providerId);
  }

  async function requestAuthenticatedFinalFollowup(task: StudioTask, providerId: string) {
    runningTaskId = task.id;
    finalFollowupError = '';
    error = '';
    fileToolBusy = true;
    try {
      const response = await invoke<unknown>('studio_continue_after_native_tool', {
        taskId: task.id, providerId
      });
      if (!response || typeof response !== 'object' || Array.isArray(response)) throw new Error('Missing native result');
      const value = response as Record<string, unknown>;
      const updated = value.task as Record<string, unknown> | undefined;
      if (!updated || updated.id !== task.id || updated.state !== 'succeeded' ||
          value.provider_id !== providerId || typeof value.result_message_id !== 'string' ||
          value.tools_executed_in_followup !== false || value.native_tool_observation_verified !== true ||
          value.automatic_resume_allowed !== false) throw new Error('Unexpected native continuation result');
      fileToolResult = null;
      fileToolOriginProviderId = '';
      notice = 'One final text-only model result was recorded after your reviewed file action. No additional tool was executed.';
    } catch {
      // A provider request might have been charged before disconnection.
      // Never offer another same-ID attempt: native revision 3 prevents it.
      fileToolOriginProviderId = '';
      fileToolResult = null;
      finalFollowupError = 'The one-use model follow-up may have been consumed. Inspect the saved task and audit status; do not retry this request.';
      error = finalFollowupError;
    } finally {
      await refresh();
      runningTaskId = '';
      fileToolBusy = false;
    }
  }

  // Renderer state is presentational only: authenticated Core/checkpoint
  // records and separately acquired native OS consent are authoritative.
  function validateMultiStepStatus(value: unknown, providerId: string): MultiStepState {
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid native step status');
    const row = value as Record<string, unknown>;
    const allowed = ['not_started', 'awaiting_model', 'awaiting_native_review', 'complete', 'cancelled', 'quarantined', 'provider_failed'];
    if (typeof row.status !== 'string' || !allowed.includes(row.status)) throw new Error('Unexpected native step status');
    if (row.provider_id !== undefined && row.provider_id !== providerId) throw new Error('Original provider differs');
    for (const field of ['revision', 'model_reservations', 'tool_steps', 'model_limit', 'tool_limit'] as const) {
      if (row[field] !== undefined && (!Number.isSafeInteger(row[field]) ||
          (row[field] as number) < 0 || (row[field] as number) > 128)) {
        throw new Error('Invalid native step counter');
      }
    }
    if (row.status === 'awaiting_native_review' && (row.phase !== 'native_review_required' ||
        (row.action !== 'read' && row.action !== 'create_new') ||
        typeof row.relative_path !== 'string' || !row.relative_path || row.relative_path.length > 1024)) {
      throw new Error('Incomplete native approval proposal');
    }
    return row as MultiStepState;
  }

  async function inspectMultiStep(task: StudioTask) {
    const providerId = multiStepProviders[task.id] || taskProviderId(task);
    if (!isNative || !providerId || multiStepBusyId || working || runningTaskId) return;
    multiStepBusyId = task.id;
    multiStepErrors = { ...multiStepErrors, [task.id]: '' };
    try {
      const result = await invoke<unknown>('studio_native_multi_step_status', { taskId: task.id, providerId });
      const status = validateMultiStepStatus(result, providerId);
      multiStepStates = { ...multiStepStates, [task.id]: status };
      if (status.status !== 'not_started') multiStepProviders = { ...multiStepProviders, [task.id]: providerId };
      if (status.status === 'quarantined') multiStepNotices = { ...multiStepNotices,
        [task.id]: 'An interrupted request or unresolved approval is quarantined. There is no automatic replay or restored permission.' };
    } catch {
      multiStepStates = { ...multiStepStates, [task.id]: { status: 'quarantined' } };
      multiStepErrors = { ...multiStepErrors,
        [task.id]: 'The authenticated step state could not be verified. Inspection did not execute a model or device action.' };
    } finally {
      multiStepBusyId = '';
    }
  }

  async function requestNextMultiStep(task: StudioTask) {
    const providerId = multiStepProviders[task.id] || taskProviderId(task);
    const current = multiStepStates[task.id];
    if (!isNative || !providerId || task.state !== 'pending' || multiStepBusyId ||
        fileToolBusy || working || runningTaskId ||
        (current && !['not_started', 'awaiting_model'].includes(current.status))) return;
    multiStepBusyId = task.id;
    multiStepErrors = { ...multiStepErrors, [task.id]: '' };
    multiStepNotices = { ...multiStepNotices, [task.id]: 'The explicit model attempt may incur a charge, even if it fails or is interrupted.' };
    try {
      const raw = await invoke<unknown>('studio_native_multi_step_next', {
        taskId: task.id, providerId,
        modelAttempts: current?.model_limit ?? multiStepMaxModelRequests,
        toolSteps: current?.tool_limit ?? multiStepMaxToolActions
      });
      const status = validateMultiStepStatus(raw, providerId);
      if (status.status === 'complete') {
        const row = raw as Record<string, unknown>;
        const finished = row.task;
        if (!finished || typeof finished !== 'object' || (finished as StudioTask).id !== task.id ||
            (finished as StudioTask).state !== 'succeeded' || typeof row.result_message_id !== 'string' ||
            typeof row.text !== 'string') throw new Error('Uncommitted final response');
        multiStepNotices = { ...multiStepNotices, [task.id]: 'The model completed the task. Its final answer was durably saved in the room.' };
      } else if (status.status === 'awaiting_native_review') {
        multiStepNotices = { ...multiStepNotices,
          [task.id]: 'The model proposed one operation. Nothing has executed. Review it in the OS-native consent dialog, or deny it.' };
      } else if (status.status === 'provider_failed') {
        multiStepNotices = { ...multiStepNotices,
          [task.id]: 'The provider attempt failed or may have been charged. No device action executed. This failed session is terminal; do not retry its uncertain request.' };
      } else throw new Error('Unexpected step response');
      multiStepStates = { ...multiStepStates, [task.id]: status };
      multiStepProviders = { ...multiStepProviders, [task.id]: providerId };
    } catch {
      multiStepStates = { ...multiStepStates, [task.id]: { status: 'quarantined' } };
      multiStepErrors = { ...multiStepErrors,
        [task.id]: 'The attempt may have incurred a charge or its checkpoint could not be authenticated. Inspect its status; do not retry blindly.' };
    } finally {
      await refresh();
      multiStepBusyId = '';
    }
  }

  async function resolveMultiStep(task: StudioTask, decision: 'review' | 'deny') {
    const status = multiStepStates[task.id];
    if (!isNative || status?.status !== 'awaiting_native_review' || task.state !== 'pending' ||
        multiStepBusyId || fileToolBusy || working || runningTaskId) return;
    multiStepBusyId = task.id;
    multiStepErrors = { ...multiStepErrors, [task.id]: '' };
    try {
      const command = decision === 'review' ? 'studio_native_multi_step_review' : 'studio_native_multi_step_deny';
      const result = await invoke<unknown>(command, { taskId: task.id });
      if (!result || typeof result !== 'object' || Array.isArray(result)) throw new Error('Invalid native result');
      const row = result as Record<string, unknown>;
      if (decision === 'deny') {
        if (row.status !== 'denied' || row.executed !== false) throw new Error('Unverified native denial');
        multiStepNotices = { ...multiStepNotices,
          [task.id]: 'The operation was denied; no device action was performed. Another model step requires a new explicit request.' };
      } else if (row.status === 'denied' && row.executed === false) {
        multiStepNotices = { ...multiStepNotices, [task.id]: 'You declined independent OS confirmation; no device action was performed.' };
      } else if (row.status === 'executed' &&
                 ['succeeded', 'failed', 'timed_out', 'output_limit', 'cancelled'].includes(String(row.outcome)) &&
                 row.next_model_requires_explicit_user_action === true && typeof row.operation_id === 'string') {
        multiStepNotices = { ...multiStepNotices,
          [task.id]: 'Native audit result: ' + String(row.outcome).replaceAll('_', ' ') +
            '. No additional model request or device operation was automatically initiated.' };
      } else throw new Error('Invalid native audit receipt');
      const providerId = multiStepProviders[task.id];
      if (!providerId) throw new Error('Original provider unavailable');
      const current = await invoke<unknown>('studio_native_multi_step_status', { taskId: task.id, providerId });
      multiStepStates = { ...multiStepStates, [task.id]: validateMultiStepStatus(current, providerId) };
    } catch {
      // A native file effect may have occurred before an IPC error. Never
      // claim no effect or replay an old one-use operation.
      multiStepStates = { ...multiStepStates, [task.id]: { status: 'quarantined' } };
      multiStepErrors = { ...multiStepErrors,
        [task.id]: 'The operation was rejected or its outcome is unverified. A file may have changed. Inspect authenticated history before taking further action.' };
    } finally {
      await refresh();
      multiStepBusyId = '';
    }
  }

  async function createBackup() {
    if (!isNative || backupBusy || working) return;
    backupCreateError = '';
    backupResultPath = '';
    if (backupPassphrase !== backupConfirmation || utf8Bytes(backupPassphrase) < 8 || utf8Bytes(backupPassphrase) > maxBackupPassphraseBytes) {
      backupCreateError = 'Passphrases must match and contain 8–1024 UTF-8 bytes.';
      return;
    }
    backupBusy = true;
    backupOperation = 'create';
    try {
      const result = await invoke<{ path: string; encrypted: boolean; includes_attachments: boolean }>('studio_create_encrypted_backup', { passphrase: backupPassphrase });
      if (result.encrypted !== true || result.includes_attachments !== false || !result.path) throw new Error('Invalid backup result');
      backupResultPath = result.path;
      notice = 'A new encrypted workspace backup was created. Keep the passphrase in a safe place.';
    } catch {
      backupCreateError = 'Could not create a backup. Check disk space, workspace size, and passphrase length.';
    } finally {
      backupPassphrase = '';
      backupConfirmation = '';
      backupBusy = false;
      backupOperation = null;
    }
  }

  async function createProjectArchive() {
    if (!isNative || backupBusy || working) return;
    projectBackupError = '';
    projectArchivePath = '';
    if (projectPassphrase !== projectConfirmation || utf8Bytes(projectPassphrase) < 8 || utf8Bytes(projectPassphrase) > maxBackupPassphraseBytes) {
      projectBackupError = 'Passphrases must match and contain 8–1024 UTF-8 bytes.';
      return;
    }
    backupBusy = true;
    backupOperation = 'project-create';
    try {
      const result = await invoke<{ path: string; encrypted: boolean; databases: string[]; includes_attachments: boolean }>(
        'studio_create_project_archive', { passphrase: projectPassphrase }
      );
      if (result.encrypted !== true || result.includes_attachments !== false || !result.path ||
        JSON.stringify(result.databases) !== JSON.stringify(['studio.sqlite3', 'workflows.sqlite3'])) {
        throw new Error('Unexpected archive contract');
      }
      projectArchivePath = result.path;
      notice = 'Encrypted archive created for your studio and workflow databases.';
    } catch (cause) {
      projectBackupError = typeof cause === 'string' && cause.startsWith('Could not create the encrypted project archive:') && cause.length < 300
        ? cause : 'Could not create the project archive. Check both databases, total size and available disk space.';
    } finally {
      projectPassphrase = '';
      projectConfirmation = '';
      backupBusy = false;
      backupOperation = null;
    }
  }

  async function restoreProjectArchive() {
    if (!isNative || backupBusy || working) return;
    projectRestoreError = '';
    projectRestoredDirectory = '';
    if (!projectRestorePath.trim() || utf8Bytes(projectRestorePath.trim()) > maxRecoveryPathBytes ||
        utf8Bytes(projectRestorePassphrase) < 8 || utf8Bytes(projectRestorePassphrase) > maxBackupPassphraseBytes) {
      projectRestoreError = 'Enter a local archive path and its original passphrase (8–1024 bytes).';
      return;
    }
    backupBusy = true;
    backupOperation = 'project-restore';
    try {
      const result = await invoke<{ directory: string; studio: string; workflows: string; active_workspace_replaced: boolean }>(
        'studio_restore_project_archive', { packagePath: projectRestorePath.trim(), passphrase: projectRestorePassphrase }
      );
      if (result.active_workspace_replaced !== false || !result.directory ||
          !result.studio.endsWith('studio.sqlite3') || !result.workflows.endsWith('workflows.sqlite3')) {
        throw new Error('Unexpected restored project contract');
      }
      projectRestoredDirectory = result.directory;
      notice = 'Both databases were authenticated and restored into a separate project directory.';
    } catch (cause) {
      projectRestoreError = typeof cause === 'string' && cause.startsWith('Could not recover the encrypted project:') && cause.length < 300
        ? cause : 'Could not authenticate or restore both databases. Check the archive, passphrase and free space.';
    } finally {
      projectRestorePassphrase = '';
      backupBusy = false;
      backupOperation = null;
    }
  }

  async function restoreBackup() {
    if (!isNative || backupBusy || working) return;
    backupRestoreError = '';
    restoredDatabasePath = '';
    if (!restorePackagePath.trim() || utf8Bytes(restorePackagePath.trim()) > maxRecoveryPathBytes || utf8Bytes(restorePassphrase) < 8 || utf8Bytes(restorePassphrase) > maxBackupPassphraseBytes) {
      backupRestoreError = 'Enter a full backup path of at most 4096 UTF-8 bytes and a passphrase of 8–1024 UTF-8 bytes.';
      return;
    }
    backupBusy = true;
    backupOperation = 'restore';
    try {
      const result = await invoke<{ path: string; active_workspace_replaced: boolean }>('studio_restore_encrypted_backup', { packagePath: restorePackagePath.trim(), passphrase: restorePassphrase });
      if (result.active_workspace_replaced !== false || !result.path) throw new Error('Invalid restore result');
      restoredDatabasePath = result.path;
      notice = 'Encrypted backup authenticated and restored into a separate SQLite database. Your active workspace was not replaced.';
    } catch {
      backupRestoreError = 'The package could not be authenticated or recovered. Check its path, passphrase, integrity, and free disk space.';
    } finally {
      restorePassphrase = '';
      backupBusy = false;
      backupOperation = null;
    }
  }

  async function copyLocalPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      notice = 'File path copied to the clipboard.';
    } catch {
      notice = '';
      if (path === backupResultPath) backupCreateError = 'Could not copy the file path. Select and copy the path shown below instead.';
      else if (path === projectArchivePath) projectBackupError = 'Could not copy the archive path. Select the text below.';
      else if (path === projectRestoredDirectory) projectRestoreError = 'Could not copy the recovery directory. Select the text below.';
      else backupRestoreError = 'Could not copy the file path. Select and copy the path shown below instead.';
    }
  }

  function clearBackupSecrets() {
    backupPassphrase = '';
    backupConfirmation = '';
    restorePassphrase = '';
    projectPassphrase = '';
    projectConfirmation = '';
    projectRestorePassphrase = '';
  }

  function toggleAdvanced() {
    if (advanced) clearBackupSecrets();
    advanced = !advanced;
  }

  function invalidateWorkflowPreview() {
    workflowPreview = null;
    validatedWorkflowJson = '';
    validatedWorkflowSignature = '';
    workflowError = '';
    workflowNotice = '';
  }

  function buildWorkflowJson(id: string): string {
    const brief = workflowBrief.trim();
    if (!brief || utf8Bytes(brief) > 1500) throw new Error('Enter a goal of up to 1,500 UTF-8 bytes.');
    if (workflowStepCount < 1 || workflowStepCount > 3) throw new Error('Choose one, two or three steps.');
    const steps = activeWorkflowSteps.map((step, index) => {
      const agent = snapshot.agents.find((entry) => entry.id === step.agentId);
      if (!agent) throw new Error(`Choose an existing agent for ${step.title}.`);
      if (!snapshot.providers.some((entry) => entry.id === step.providerId)) {
        throw new Error(`Choose a model provider explicitly for ${step.title}.`);
      }
      const instruction = `${step.instruction.trim()}\n\nGoal: ${brief}`;
      if (!step.instruction.trim() || utf8Bytes(instruction) > 4096) {
        throw new Error(`The ${step.title} instructions must fit in 4,096 UTF-8 bytes including the goal.`);
      }
      return {
        id: step.id, role: agent.role, agent_id: agent.id, provider_id: step.providerId,
        instruction, depends_on: index === 0 ? [] : [activeWorkflowSteps[index - 1].id],
        max_attempts: 2,
      };
    });
    return JSON.stringify({ schema_version: 1, workflow_id: id, steps });
  }

  async function loadWorkflows(targetOffset = workflowOffset, searchTerm = workflowAppliedSearch) {
    if (!isNative || workflowLoading || workflowBusy || workflowCancelBusy ||
        !Number.isSafeInteger(targetOffset) || targetOffset < 0 ||
        targetOffset > 100_000 || targetOffset % workflowPageSize !== 0) return;
    if (new TextEncoder().encode(searchTerm).length > 128 || /[\x00-\x1f\x7f]/.test(searchTerm)) {
      workflowError = 'Search must be at most 128 UTF-8 bytes, without control characters.';
      return;
    }
    workflowLoading = true;
    workflowError = '';
    try {
      const response = await invoke<WorkflowListResponse>('studio_list_workflows', { offset: targetOffset, query: searchTerm });
      if (!Array.isArray(response.items) || response.items.length > workflowPageSize ||
          response.max_items !== workflowPageSize || response.offset !== targetOffset || response.query !== searchTerm ||
          typeof response.has_more !== 'boolean' ||
          (response.has_more && response.items.length !== workflowPageSize) ||
          !response.items.every((item) => item.definition && typeof item.definition.workflow_id === 'string' &&
            Array.isArray(item.definition.steps) && Array.isArray(item.ready_step_ids) &&
            Number.isSafeInteger(item.revision) && typeof item.cancelled === 'boolean')) {
        throw new Error('Invalid workflow list response');
      }
      savedWorkflows = response.items;
      workflowOffset = response.offset;
      workflowAppliedSearch = response.query;
      workflowHasMore = response.has_more;
      workflowLoaded = true;
    } catch {
      workflowError = 'Saved workflows could not be loaded from this device. Try refreshing.';
    } finally {
      workflowLoading = false;
    }
  }

  function showWorkflows() {
    tasksView = 'workflows';
    invalidateWorkflowPreview();
    void loadWorkflows();
  }

  async function previewWorkflow() {
    if (!isNative || workflowBusy) return;
    invalidateWorkflowPreview();
    const id = workflowDraftId || `workflow-${crypto.randomUUID()}`;
    let definitionJson: string;
    try { definitionJson = buildWorkflowJson(id); }
    catch (cause) { workflowError = String(cause); return; }
    workflowDraftId = id;
    const signature = workflowSignature;
    workflowBusy = true;
    try {
      const response = await invoke<WorkflowPreview>('studio_preview_workflow', { definitionJson });
      const ids = activeWorkflowSteps.map((step) => step.id);
      if (response.workflow_id !== id || response.step_count !== ids.length ||
          response.preview_only !== true || response.provider_requests_sent !== false ||
          response.tools_executed !== false || response.stored !== false ||
          !Array.isArray(response.ordered_step_ids) || !Array.isArray(response.ready_step_ids) ||
          response.ordered_step_ids.join(',') !== ids.join(',') ||
          response.ready_step_ids.join(',') !== ids[0]) {
        throw new Error('Unexpected local workflow preview');
      }
      if (workflowSignature === signature) {
        workflowPreview = response;
        validatedWorkflowJson = definitionJson;
        validatedWorkflowSignature = signature;
      }
    } catch {
      workflowError = 'The plan could not be validated. Check each agent, provider and instruction.';
    } finally {
      workflowBusy = false;
    }
  }

  async function saveWorkflow() {
    if (!isNative || workflowBusy || !workflowPreviewCurrent) return;
    workflowError = '';
    workflowNotice = '';
    const definitionJson = validatedWorkflowJson;
    const id = workflowPreview!.workflow_id;
    workflowBusy = true;
    try {
      const response = await invoke<WorkflowCreateResponse>('studio_create_workflow', { definitionJson });
      if (response.workflow_id !== id || response.stored !== true || response.executed !== false ||
          response.tools_executed !== false || !Number.isSafeInteger(response.revision)) {
        throw new Error('Invalid native save confirmation');
      }
      workflowNotice = 'Workflow saved locally. No AI requests or device tools started.';
      // Reveal the newly saved record even when the user had been browsing a
      // different history page or filtering by an unrelated identifier.
      workflowOffset = 0;
      workflowSearchQuery = id;
      workflowAppliedSearch = id;
      workflowDraftId = '';
      workflowPreview = null;
      validatedWorkflowJson = '';
      validatedWorkflowSignature = '';
    } catch {
      workflowError = 'Could not confirm the workflow was saved. Check the list before retrying the same plan.';
    } finally {
      workflowBusy = false;
      await loadWorkflows();
    }
  }

  async function cancelWorkflow(record: SavedWorkflow) {
    if (!isNative || workflowCancelBusy || record.cancelled) return;
    if (workflowCancelConfirmation !== record.definition.workflow_id) {
      workflowCancelConfirmation = record.definition.workflow_id;
      return;
    }
    workflowError = '';
    workflowNotice = '';
    workflowCancelBusy = true;
    try {
      const response = await invoke<WorkflowCancelResponse>('studio_cancel_workflow', {
        workflowId: record.definition.workflow_id, expectedRevision: record.revision
      });
      if (response.workflow_id !== record.definition.workflow_id || response.cancelled !== true ||
          !Number.isSafeInteger(response.revision) || response.revision < record.revision) {
        throw new Error('Invalid native cancellation confirmation');
      }
      workflowNotice = 'Pending steps cancelled. If a model request was already running, the provider may have received it; no late result will be saved.';
      workflowCancelConfirmation = '';
    } catch {
      workflowError = 'Cancellation could not be confirmed. Refresh the list; another window may have changed this workflow.';
      workflowCancelConfirmation = '';
    } finally {
      workflowCancelBusy = false;
      await loadWorkflows();
    }
  }

  async function runWorkflowStep(record: SavedWorkflow, stepId: string) {
    if (!isNative || workflowBusy || record.cancelled || !record.ready_step_ids.includes(stepId)) return;
    workflowError = '';
    workflowNotice = '';
    workflowBusy = true;
    try {
      const result = await invoke<WorkflowStepResponse>('studio_run_workflow_step', {
        workflowId: record.definition.workflow_id, stepId, expectedRevision: record.revision
      });
      if (result.workflow_id !== record.definition.workflow_id ||
          result.step_id !== stepId || result.status !== 'succeeded' ||
          result.provider_requests_sent !== true || result.tools_executed !== false ||
          typeof result.result_text !== 'string' ||
          !Number.isSafeInteger(result.revision) || result.revision <= record.revision) {
        throw new Error('Invalid workflow completion confirmation');
      }
      workflowNotice = 'Step completed and its text result was saved locally. Run the next ready step when you choose.';
    } catch {
      workflowError = 'The step did not return confirmed success. A provider request may have been sent. Refresh the workflow to inspect its saved status before retrying.';
    } finally {
      workflowBusy = false;
      await loadWorkflows();
    }
  }

  async function copyWorkflowReport(record: SavedWorkflow) {
    if (!isNative || workflowBusy) return;
    workflowError = '';
    try {
      const report = await invoke<string>('studio_export_workflow_markdown', {
        workflowId: record.definition.workflow_id
      });
      if (!report.startsWith('# Constellation workflow report')) throw new Error('Invalid report');
      await navigator.clipboard.writeText(report);
      workflowNotice = 'Workflow report and saved step results copied as Markdown.';
    } catch {
      workflowError = 'Could not copy the saved workflow report to your clipboard.';
    }
  }

  async function viewWorkflowReport(record: SavedWorkflow) {
    if (!isNative || workflowBusy) return;
    const id = record.definition.workflow_id;
    if (displayedWorkflowReportId === id) {
      displayedWorkflowReportId = '';
      displayedWorkflowMarkdown = '';
      return;
    }
    workflowError = '';
    workflowBusy = true;
    try {
      const report = await invoke<string>('studio_export_workflow_markdown', { workflowId: id });
      if (!report.startsWith('# Constellation workflow report\n') || !report.includes('Workflow ID: `' + id + '`\n')) {
        throw new Error('Unexpected report contract');
      }
      displayedWorkflowMarkdown = report;
      displayedWorkflowReportId = id;
    } catch {
      displayedWorkflowMarkdown = '';
      displayedWorkflowReportId = '';
      workflowError = 'Could not read the saved workflow report.';
    } finally {
      workflowBusy = false;
    }
  }

  async function retryWorkflowStep(record: SavedWorkflow, stepId: string) {
    if (!isNative || workflowBusy || record.cancelled || record.states?.[stepId]?.status !== 'awaiting_retry') return;
    workflowBusy = true;
    workflowError = '';
    workflowNotice = '';
    try {
      const result = await invoke<{ workflow_id: string; revision: number; provider_requests_sent: boolean }>('studio_retry_workflow_step', {
        workflowId: record.definition.workflow_id, stepId, expectedRevision: record.revision
      });
      if (result.workflow_id !== record.definition.workflow_id ||
          result.provider_requests_sent !== false ||
          !Number.isSafeInteger(result.revision) || result.revision <= record.revision) {
        throw new Error('Invalid retry confirmation');
      }
      workflowNotice = 'Retry enabled locally. Select Run when you want to send another model request.';
    } catch {
      workflowError = 'Retry state could not be saved. Refresh the workflow before trying again.';
    } finally {
      workflowBusy = false;
      await loadWorkflows();
    }
  }

  async function runWorkflow(record: SavedWorkflow) {
    if (!isNative || workflowBusy || record.cancelled || !record.ready_step_ids.length) return;
    workflowBusy = true;
    workflowError = '';
    workflowNotice = '';
    try {
      const result = await invoke<WorkflowRunResponse>('studio_run_workflow', {
        workflowId: record.definition.workflow_id, expectedRevision: record.revision
      });
      if (result.workflow_id !== record.definition.workflow_id ||
          !Array.isArray(result.executed_step_ids) || result.executed_step_ids.length < 1 ||
          result.executed_step_ids.length > 3 || result.provider_requests_sent !== true ||
          result.tools_executed !== false || !Number.isSafeInteger(result.revision) ||
          result.revision <= record.revision) {
        throw new Error('Invalid native workflow run result');
      }
      workflowNotice = `${result.executed_step_ids.length} text step(s) completed, saved and attributed. ${result.finished ? 'Workflow finished.' : 'Refresh to review remaining steps.'}`;
    } catch {
      workflowError = 'The workflow stopped without confirming all steps. Completed steps remain saved; refresh to inspect exact status. Model requests may have been sent.';
    } finally {
      workflowBusy = false;
      await loadWorkflows();
    }
  }

  async function saveProvider() {
    if (!isNative || working || !providerName.trim() || !providerEndpoint.trim() || !providerModel.trim()) return;
    if (providerRevisionSourceId && providerRevisionSavedKey && !providerKey.trim()) {
      error = 'This connection has a remembered key. Enter a replacement API key before saving a revision, or close the dialog without changing it.';
      return;
    }
    if (providerRememberKey && !providerKey.trim()) {
      error = 'Enter an API key before choosing secure OS credential storage.';
      return;
    }
    working = true; error = '';
    const persistInOs = providerRememberKey;
    try {
      await invoke('studio_save_provider', {
        name: providerName.trim(), endpoint: providerEndpoint.trim(), model: providerModel.trim(), apiKey: providerKey,
        providerKind, rememberKey: persistInOs,
      });
      providerKey = '';
      providerRememberKey = false;
      // A previous probe is invalid after profile or in-memory key changes.
      probeResults = {};
      const wasRevision = !!providerRevisionSourceId;
      await refresh(); dialog = null;
      providerRevisionSourceId = '';
      providerRevisionSavedKey = false;
      notice = persistInOs
        ? `${providerPreset.label} saved. Its API key was stored in this operating system account's credential manager; no model request was sent.`
        : `${providerPreset.label} profile saved locally. No request was sent; any API key is session-only and may need re-entry after restart.`;
      if (wasRevision) notice += ' If endpoint, name, model or format changed, the original profile remains available until separately retired.';
    } catch (cause) { error = String(cause); }
    finally { working = false; }
  }

  async function testProvider(providerId: string) {
    if (!isNative || probeBusyId || working) return;
    probeBusyId = providerId;
    const previous = { ...probeResults };
    delete previous[providerId];
    probeResults = previous;
    try {
      const result = await invoke<ProviderProbe>('studio_probe_provider', { providerId });
      if (result.provider_id !== providerId || typeof result.status !== 'string' || result.text_chat_probe !== true) {
        throw new Error('Probe result did not match the selected provider');
      }
      probeResults = { ...probeResults, [providerId]: result };
    } catch {
      // A diagnostic from native IPC could contain URL/credential context.
      probeResults = { ...probeResults, [providerId]: { status: 'request_error', message: 'Provider test could not be completed. Review your connection and try again.' } };
    } finally {
      probeBusyId = '';
    }
  }

  async function removeUnusedProvider(provider: Provider) {
    if (!isNative || working || replyBusy || workflowBusy || !!probeBusyId || provider.remembered_key || providerRemovalCandidate !== provider.id) return;
    working = true; error = '';
    try {
      await invoke('studio_remove_unused_provider', {
        providerId: provider.id,
        expectedName: provider.name,
        expectedEndpoint: provider.endpoint,
        expectedModel: provider.model,
        expectedKind: provider.kind ?? 'openai_chat',
      });
      if (selectedProviderId === provider.id) selectedProviderId = '';
      const results = { ...probeResults }; delete results[provider.id]; probeResults = results;
      providerRemovalCandidate = '';
      await refresh();
      notice = 'Unused provider removed from this local workspace. No network request was sent.';
    } catch {
      providerRemovalCandidate = '';
      error = 'Removal was denied. The connection may have changed or still be referenced by an agent, a task, a message, a workflow or stored credentials. Nothing was removed; check your assignments and history.';
    } finally { working = false; }
  }
  async function forgetRememberedProviderKey(provider: Provider) {
    if (!isNative || working || replyBusy || workflowBusy || !!probeBusyId ||
        !provider.remembered_key || providerForgetCandidate !== provider.id ||
        providerForgetConfirmation !== provider.name) return;
    working = true; error = '';
    try {
      await invoke('studio_forget_remembered_provider_key', {
        providerId: provider.id,
        expectedName: provider.name,
        expectedEndpoint: provider.endpoint,
        expectedModel: provider.model,
        expectedKind: provider.kind ?? 'openai_chat',
        confirmedName: providerForgetConfirmation,
      });
      providerForgetCandidate = '';
      providerForgetConfirmation = '';
      const results = { ...probeResults }; delete results[provider.id]; probeResults = results;
      await refresh();
      notice = 'Saved API key deleted from the operating system credential manager. Connection identity and history were preserved. Enter a new key to request further replies.';
    } catch {
      providerForgetCandidate = '';
      providerForgetConfirmation = '';
      error = 'Could not verify key deletion and profile update. No request was sent. Reopen Settings to inspect the current credential status before using this provider.';
    } finally { working = false; }
  }

  function openTaskDialog(preferredRoomId?: string) {
    taskSetupReturn = null;
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
    if (taskCancelConfirmation !== taskId) {
      taskCancelConfirmation = taskId;
      return;
    }
    working = true; error = '';
    try {
      await invoke('studio_cancel_task', { taskId });
      await refresh();
      notice = 'Cancellation recorded. An already dispatched model request cannot be recalled.';
    } catch (cause) { error = `Could not cancel task: ${String(cause)}`; }
    finally { taskCancelConfirmation = ''; working = false; }
  }

  async function runTask(taskId: string) {
    const task = snapshot.tasks.find((entry) => entry.id === taskId);
    const providerId = task ? taskProviderId(task) : '';
    if (!isNative || !providerId || runningTaskId || working) return;
    runningTaskId = taskId;
    error = '';
    try {
      await invoke('studio_run_task', { taskId, providerId });
      notice = 'Model task finished. Its response is saved in the associated room.';
    } catch (cause) {
      error = `Task was not completed: ${String(cause)}`;
    } finally {
      await refresh();
      runningTaskId = '';
    }
  }

  function taskProviderId(task: StudioTask): string {
    const agent = snapshot.agents.find((entry) => entry.id === task.agent_id);
    // A task's explicitly selected model cannot silently change when the user
    // switches chat rooms or changes the model for another task.
    const choice = taskProviders[task.id] ?? '';
    const id = choice === 'each-agent' ? agent?.provider_id : choice;
    return id && snapshot.providers.some((profile) => profile.id === id) ? id : '';
  }

  function keyboard(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    const target = event.target;
    const editing = target instanceof HTMLElement && !!target.closest('input, textarea, select, [contenteditable="true"]');
    if (!dialog && !editing && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey) {
      const sections: Screen[] = ['home', 'rooms', 'tasks', 'evidence', 'library', 'settings'];
      const index = Number(event.key) - 1;
      if (Number.isInteger(index) && index >= 0 && index < sections.length) {
        event.preventDefault();
        navigate(sections[index]);
        return;
      }
      if (event.key === ',') {
        event.preventDefault();
        navigate('settings');
        return;
      }
    }
    if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'f' && screen === 'rooms' && !dialog) {
      event.preventDefault();
      focusMode = !focusMode;
      if (focusMode) { showDetails = false; sidebarOpen = false; }
      return;
    }
    if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      // Do not discard another modal's unsaved form state or move focus to a
      // newly replaced field inside the same dialog node.
      if (!dialog) dialog = 'commands';
      if (!dialog || dialog === 'commands') void focusCurrentDialogField();
      return;
    }
    if (event.key === 'Escape') {
      if (dialog) closeDialog();
      else if (focusMode) focusMode = false;
      else if (sidebarOpen) {
        sidebarOpen = false;
        document.querySelector<HTMLButtonElement>('.mobile-menu')?.focus();
      } else showDetails = false;
    }
  }

  function focusDialog(node: HTMLElement) {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // Palette options are controlled by aria-activedescendant and intentionally
    // tabindex=-1; they must not be counted as Tab stops in the modal trap.
    const selector = 'button:not([disabled]):not([tabindex="-1"]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
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

  function navigate(next: Screen) {
    if (screen === 'settings' && next !== 'settings') clearBackupSecrets();
    screen = next;
    if (next !== 'rooms') focusMode = false;
    closeDialog();
    sidebarOpen = false;
  }
  function openCommandAction(action: WorkspaceAction) {
    dialog = action;
    void focusCurrentDialogField();
  }
  function continueTaskSetup(action: 'new-room' | 'new-agent') {
    taskSetupReturn = action;
    dialog = action;
    void focusCurrentDialogField();
  }
  function openCommandRoom(id: string) {
    selectRoom(id);
    navigate('rooms');
  }
</script>

<svelte:window onkeydown={keyboard} />
<svelte:head><title>Constellation — Your AI team, in your space</title><meta name="description" content="A local-first workspace for collaborating with your AI team" /></svelte:head>

<a class="skip-workspace-link" href="#workspace-main">Skip to workspace content</a>
<div class="studio" class:focus-mode={focusMode && screen === 'rooms'}>
  <nav class="rail" aria-label="Main navigation">
    <div class="wordmark" title="Constellation"><div class="mark">✳</div></div>
    <div class="rail-items">
      <button class:active={screen === 'home'} aria-current={screen === 'home' ? 'page' : undefined} onclick={() => navigate('home')} title="Home" aria-label="Home"><Activity size={20}/></button>
      <button class:active={screen === 'rooms'} aria-current={screen === 'rooms' ? 'page' : undefined} onclick={() => navigate('rooms')} title="Rooms" aria-label="Rooms"><MessageCircle size={20}/></button>
      <button class:active={screen === 'tasks'} aria-current={screen === 'tasks' ? 'page' : undefined} onclick={() => navigate('tasks')} title="Tasks" aria-label="Tasks"><Layers3 size={20}/></button>
      <button class:active={screen === 'evidence'} aria-current={screen === 'evidence' ? 'page' : undefined} onclick={() => navigate('evidence')} title="Evidence" aria-label="Evidence"><FileSearch size={20}/></button>
      <button class:active={screen === 'library'} aria-current={screen === 'library' ? 'page' : undefined} onclick={() => navigate('library')} title="Library" aria-label="Library"><BookOpen size={20}/></button>
    </div>
    <div class="rail-bottom">
      <button onclick={() => (dialog = 'commands')} title="Search workspace (Ctrl+K)" aria-label="Search workspace (Ctrl+K)"><Command size={19}/></button>
      <button class:active={screen === 'settings'} aria-current={screen === 'settings' ? 'page' : undefined} onclick={() => navigate('settings')} title="Settings" aria-label="Settings"><Settings2 size={20}/></button>
      <div class="profile" title="Local workspace" aria-label="Local workspace">Y</div>
    </div>
  </nav>

  <aside class:open={sidebarOpen} class="sidebar" aria-label="Workspace sidebar">
    <div class="workspace-heading"><div class="workspace-icon">✧</div><div><strong>My workspace</strong><span>Personal space</span></div><ChevronDown size={15}/></div>
    {#if screen === 'rooms'}
      <div class="sidebar-section-title"><span>CONVERSATIONS</span><button aria-label="Create room" title="Create room" onclick={() => (dialog = 'new-room')}><Plus size={17}/></button></div>
      {#if snapshot.rooms.length > 0}
        <label class="room-search"><Search size={15} aria-hidden="true"/><input type="search" aria-label="Filter rooms" placeholder="Find a conversation…" autocomplete="off" bind:value={roomSearch}/><kbd>Ctrl K</kbd></label>
      {/if}
      <div class="room-list">
        {#each filteredRooms as r (r.id)}
          <button class:selected={roomId === r.id} aria-current={roomId === r.id ? 'true' : undefined} class="room-link" onclick={() => selectRoom(r.id)}><span class="room-symbol">#</span><span>{r.name}</span></button>
        {:else}
          <div class="sidebar-empty">{snapshot.rooms.length ? 'No rooms match. Clear the search to see all conversations.' : 'Rooms you create appear here.'}</div>
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
      <div class="section-context">{screen === 'home' ? 'A place to begin' : screen === 'tasks' ? 'Work in progress' : screen === 'evidence' ? 'Private document search' : screen === 'library' ? 'People and capabilities' : 'Your preferences'}</div>
      <div class="sidebar-submenu"><button onclick={() => navigate('rooms')}><MessageCircle size={16}/> Back to rooms <ArrowRight size={15}/></button></div>
    {/if}
    <div class="sidebar-footer"><div class:inactive={!isNative} class="local-dot"></div><div><strong>{!runtimeChecked ? 'Checking local runtime' : isNative ? 'Workspace on this device' : 'Interface preview'}</strong><span>{!runtimeChecked ? 'Checking availability…' : isNative ? 'Local app — no operator cloud' : 'Desktop app needed to save'}</span></div><ShieldCheck size={16}/></div>
  </aside>

  <main class="main" id="workspace-main" tabindex="-1">
    {#if runtimeChecked && !isNative}<div class="preview-banner" role="status"><Cpu size={16}/> Browser preview: explore the interface here. Create, save, and send actions require the installed desktop app.</div>{/if}
    {#if notice}<div class="toast" role="status"><Check size={15}/>{notice}<button aria-label="Dismiss notice" onclick={() => (notice = '')}><X size={14}/></button></div>{/if}
    {#if error}<div class="error-toast" role="alert">{error}<button aria-label="Dismiss error" onclick={() => (error = '')}><X size={15}/></button></div>{/if}

    {#if screen === 'rooms'}
      <header class="topbar">
        <button class="mobile-menu icon-button" aria-label="Show rooms" aria-expanded={sidebarOpen} onclick={() => (sidebarOpen = !sidebarOpen)}><Menu size={20}/></button>
        <div class="crumb"><span class="crumb-muted">Rooms</span><ChevronRight size={15}/><strong>{room?.name || 'Your first space'}</strong></div>
        <div class="top-actions"><span class="privacy-note"><ShieldCheck size={14}/> Local by default</span>{#if room}<button class="subtle-cta room-task-action" onclick={() => openTaskDialog(roomId)}><Plus size={15}/> Plan task</button>{/if}<button class="icon-button" type="button" aria-label={focusMode ? 'Exit focus mode' : 'Enter focus mode'} aria-pressed={focusMode} title={focusMode ? 'Exit focus mode (Esc)' : 'Focus mode (Ctrl+Shift+F)'} onclick={() => { focusMode = !focusMode; if (focusMode) { showDetails = false; sidebarOpen = false; } }}>{#if focusMode}<Minimize2 size={19}/>{:else}<Maximize2 size={19}/>{/if}</button><button class="icon-button" aria-label="Room details" aria-expanded={showDetails} title="Room details" onclick={() => { if (focusMode) focusMode = false; showDetails = !showDetails; }}><UsersRound size={19}/></button><button class="icon-button" aria-label="More room options" title="More room options" onclick={() => (dialog = 'commands')}><MoreHorizontal size={19}/></button></div>
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
          <div class="chat-scroll" bind:this={chatScrollElement} onscroll={chatScrollChanged} role="region" aria-label="Conversation history and search">
            <div class="room-intro" class:compact={messages.length > 0}><div class="room-intro-icon">#</div><h1>{room.name}</h1><p>{room.description || 'A space for you and your AI teammates to collaborate.'}</p><button class="soft-button small" onclick={() => (dialog = 'new-agent')}><Plus size={15}/> Add a teammate</button></div>
            <div class="message-search-bar">
              <label for="conversation-message-search">Search this conversation</label>
              <input id="conversation-message-search" type="search" bind:value={messageQuery} oninput={() => (messagePage = 0)} placeholder="Find messages by text or sender…" autocomplete="off" />
              {#if messageQuery}<button type="button" class="soft-button small" onclick={() => { messageQuery = ''; messagePage = 0; }}>Clear search</button>{/if}
              <span role="status">{messageQuery.trim() ? `${matchingMessages.length} of ${messages.length} matching messages` : `${messages.length} messages`}</span>
            </div>
            {#if !chatNearBottom || unreadMessageCount > 0 || messagePage > 0}
              <div class="chat-jump-toolbar"><button type="button" class="soft-button small" onclick={() => void jumpToLatestMessage()}>Jump to latest{unreadMessageCount > 0 ? ` · ${unreadMessageCount} new` : ''}</button></div>
            {/if}
            {#if matchingMessages.length > messagePageSize}
              <div class="message-pagination" role="group" aria-label="Conversation message pages">
                <button type="button" class="soft-button small" disabled={messagePage >= maxMessagePage} onclick={() => void showMessagePage(messagePage + 1)}>Older messages</button>
                <span>Showing {Math.max(1, visibleMessageEnd - visibleMessages.length + 1)}–{visibleMessageEnd} of {matchingMessages.length}</span>
                <button type="button" class="soft-button small" disabled={messagePage === 0} onclick={() => void showMessagePage(messagePage - 1)}>Newer messages</button>
              </div>
            {/if}
            <div role="log" aria-label="Conversation messages" aria-live="off">
            {#each visibleMessages as message (message.id)}
              <article class="message" data-message-id={message.id}><div class:user-avatar={message.sender_kind === 'user'} class="message-avatar">{message.sender_kind === 'user' ? 'Y' : '✳'}</div><div class="message-body"><div class="message-byline"><strong>{message.sender_name}</strong><span>{timeDisplay(message.created_at)}</span>{#if message.simulated}<span class="simulated">Simulation</span>{/if}<button type="button" class="message-copy" aria-label={'Copy message from ' + message.sender_name} onclick={() => void copyMessageText(message.body)}>Copy message</button></div>
                <div class="message-content">
                  {#each splitChatBlocks(message.body) as block, index (index)}
                    {#if block.kind === 'code'}
                      <div class="message-code-block"><div class="message-code-heading"><span>{block.language || 'Code'}</span><button type="button" aria-label={'Copy ' + (block.language || 'code') + ' block'} onclick={() => void copyMessageText(block.text)}>Copy code</button></div><pre><code>{block.text}</code></pre></div>
                    {:else}<div class="message-text">{block.text}</div>{/if}
                  {/each}
                </div>
                </div>
              </article>
            {:else}
              {#if messageQuery.trim()}<p class="message-search-empty" role="status">No messages match this search. Clear the query to see your conversation.</p>{/if}
            {/each}
            </div>
          </div>
          <div class="composer-container">
            <form class="composer" onsubmit={(event) => {event.preventDefault(); void sendMessage();}}>
              <textarea bind:value={draft} disabled={busy || replyBusy || !isNative || !draftReady} aria-label="Write a local message" placeholder={draftReady ? 'Write a message to this room...' : 'Loading saved drafts...'} rows="2" oninput={(event) => changeComposerDraft(event.currentTarget.value)} onblur={() => { if (draftReady && roomId) void draftPersistence.flush(roomId); }} onkeydown={(event) => {if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {event.preventDefault(); void sendMessage();}}}></textarea>
              <div class="compose-bottom"><span class="save-local-label"><ShieldCheck size={14}/> Save in this room</span><button class="send-button" type="submit" disabled={!draft.trim() || busy || replyBusy || !isNative || !draftReady} title="Save message locally" aria-label="Save message locally">{#if busy}<Square size={16}/>{:else}<ArrowRight size={19}/>{/if}</button></div>
            </form>
            {#if draftLoadError}
              <div class="draft-save-feedback error" role="alert"><span>{draftLoadError}</span><button type="button" class="soft-button small" onclick={() => void hydrateComposerDrafts()}>Retry loading drafts</button></div>
            {:else if draftReady}
              <div class="draft-save-feedback" role="status" aria-live="polite">
                <span>{draftStates[roomId] === 'saving' ? 'Saving draft locally…' : draftStates[roomId] === 'unsaved' ? 'Draft has changes not yet saved' : draftStates[roomId] === 'error' ? 'Draft not saved — your text is still in this window' : 'Draft saved locally in this project'}</span>
                {#if draftStates[roomId] === 'error'}
                  <span class="draft-save-error">{draftErrors[roomId]}</span>
                  <button type="button" class="soft-button small" onclick={() => void draftPersistence.flush(roomId)}>Retry save</button>
                  <button type="button" class="soft-button small" onclick={() => void restoreSavedDraft()}>Discard local changes and reload saved draft</button>
                {/if}
              </div>
            {/if}
            <div class="reply-panel" aria-label="Request AI replies">
              <div class="reply-row">
                <label class="reply-provider">Model provider
                  <select bind:value={selectedProviderId} disabled={!isNative || busy || replyBusy || snapshot.providers.length === 0} aria-label="Choose AI provider for replies">
                    <option value="">Choose a provider…</option>
                    <option value="each-agent">Use each agent’s saved model</option>
                    {#each snapshot.providers as provider (provider.id)}<option value={provider.id}>{provider.name} · {provider.model} ({providerKindLabel(provider.kind)})</option>{/each}
                  </select>
                </label>
                <div class="reply-actions"><button type="button" class="soft-button small" disabled={!canAsk} onclick={() => void askAgents('selected')}>Ask selected</button><button type="button" class="primary-button small" disabled={!canAsk || selectedAgentIds.length > 8} onclick={() => void askAgents('team')}>Ask team ({selectedAgentIds.length})</button></div>
              </div>
              {#if snapshot.agents.length > 0}
                <details class="reply-agent-picker"><summary>Agents · {selectedAgentIds.length} selected <span>{unsavedRoomTeam ? 'Unsaved team' : 'Change room team'}</span></summary>
                  <fieldset class="reply-agents" disabled={!isNative || busy || replyBusy}>
                    <legend>Choose one or more agents</legend>
                    <div class="agent-options">{#each snapshot.agents as agent (agent.id)}<label class:chosen={selectedAgentIds.includes(agent.id)}><input type="checkbox" checked={selectedAgentIds.includes(agent.id)} onchange={(event) => toggleAgent(agent.id, event.currentTarget.checked)}/><span>{agent.name}</span></label>{/each}</div>
                  </fieldset>
                  <div class="room-team-controls">
                    <span role="status">{unsavedRoomTeam ? 'Changes apply to this room now but are not yet saved for the next launch.' : room?.members_configured ? 'This room team is saved on this device.' : 'This older room has no saved team yet; the first agent is suggested.'}</span>
                    {#if unsavedRoomTeam}
                      <button type="button" class="soft-button small" disabled={!isNative || working || busy || replyBusy || selectedAgentIds.length > 8} onclick={() => void saveRoomTeam()}>Save room team</button>
                      <button type="button" class="text-button" disabled={working} onclick={() => { const next = { ...selectedAgentsByRoom }; delete next[roomId]; selectedAgentsByRoom = next; }}>Discard changes</button>
                    {/if}
                  </div>
                </details>
                <label class="reply-stream-toggle"><input type="checkbox" bind:checked={streamAnswers} disabled={!isNative || replyBusy} /><span>Show a live text preview as the model responds (supported providers)</span></label>
                <p class="reply-help">Ask selected contacts the first checked agent. Ask team contacts every checked agent (up to 8). Save the room team to keep your selection after reopening. Team membership is a routing preference, not access control or a tool permission. Your room context will be sent to every provider you explicitly contact.</p>
              {:else}<p class="reply-help">No agents yet. <button type="button" class="text-button" onclick={() => (dialog = 'new-agent')}>Add a teammate</button> before requesting AI replies.</p>{/if}
              {#if snapshot.providers.length === 0}<p class="reply-help">No model providers configured. <button type="button" class="text-button" onclick={openProviderDialog}>Connect a model</button> to ask your team.</p>{/if}
              {#if usingAgentProviders && !assignedProvidersReady}<p class="reply-warning" role="status">Assign an existing model to every selected agent in Settings before using individual model routing.</p>{/if}
              {#if selectedAgentIds.length > 8}<p class="reply-warning" role="status">Choose at most 8 agents for one team request.</p>{/if}
              {#if replyBusy}<p class="reply-help" role="status">Requesting replies from {usingAgentProviders ? 'assigned agent models' : selectedProvider?.name ?? 'the chosen provider'}… Each agent may have a different outcome.</p>{/if}
              {#if replyBusy && streamAnswers && streamingRoomId === roomId}
                <button type="button" class="soft-button small" disabled={replyCancelBusy || streamStopRequested} onclick={() => void stopStreamingReply()}>{replyCancelBusy || streamStopRequested ? 'Stopping stream…' : 'Stop live response'}</button>
              {/if}
              {#if replyFeedbackByRoom[roomId] && replyFeedbackByRoom[roomId].requestedIds.length > 0}
                {@const report = replyFeedbackByRoom[roomId]}
                <div class="reply-results" aria-label="Latest AI reply results" role="status"><strong>Latest request · {report.providerName}</strong>
                  {#if report.error}<p class="reply-warning">{report.error}</p>{/if}
                  <ul>{#each report.requestedIds as id (id)}
                    {@const outcome = report.replies.find((reply) => reply.agent_id === id)}
                    <li><span>{snapshot.agents.find((agent) => agent.id === id)?.name ?? 'Unknown agent'}</span><span class:reply-good={outcome?.status === 'succeeded'} class:reply-warning={outcome?.status === 'failed'}>{outcome?.status === 'succeeded' ? 'Replied' : outcome?.status === 'failed' ? 'Failed' : outcome?.status === 'skipped' ? `Skipped${outcome.reason ? ` (${outcome.reason.replace('_', ' ')})` : ''}` : 'No confirmed result'}{#if outcome?.error} — {outcome.error}{/if}</span></li>
                  {/each}</ul>
                  {#if replyBusy && streamAnswers}
                    {#each report.requestedIds as id (id)}
                      {@const preview = streamPreviewByRoom[roomId]?.[id]}
                      {#if preview}
                        <div class="reply-stream-preview" aria-label={'Provisional text for ' + (snapshot.agents.find((agent) => agent.id === id)?.name ?? 'agent')}>
                          <strong>Live preview · awaiting final verification</strong>
                          <pre>{preview}</pre>
                          <small>Recent text only. This preview has not been saved and may be discarded if the provider fails.</small>
                        </div>
                      {/if}
                    {/each}
                  {/if}
                </div>
              {/if}
              <p class="composer-footnote">{!isNative ? 'Browser preview only — open the installed desktop app.' : snapshot.providers.length === 0 ? 'Local messages work without a model.' : !selectedProvider && !usingAgentProviders ? 'Choose a provider to explicitly allow a model request.' : usingAgentProviders && !assignedProvidersReady ? 'Assign models to selected agents in Settings.' : !messages.some((m) => m.sender_kind === 'user') ? 'Save a message locally before requesting a reply.' : 'AI requests may send private room context to each selected provider. Already answered agents are skipped on retries.'}</p>
            </div>
          </div>
        {/if}
      </div>
    {:else if screen === 'home'}
      <header class="topbar"><div class="crumb"><strong>Home</strong></div><span class="privacy-note"><ShieldCheck size={14}/> Your own space</span></header>
      <div class="content-page home-page">
        <div class="eyebrow">WELCOME TO CONSTELLATION</div><h1>What will we<br /><em>work on today?</em></h1>
        <p class="page-lede">Start a focused conversation, assemble a small team, and stay in control of every action.</p>
        <div class="action-grid"><button onclick={() => (dialog = 'new-room')}><MessageCircle size={22}/><strong>Start a conversation</strong><span>Make room for a new idea</span><ArrowRight size={18}/></button><button onclick={() => navigate('library')}><UsersRound size={22}/><strong>Find a specialist</strong><span>Choose a helpful AI role</span><ArrowRight size={18}/></button><button onclick={openProviderDialog}><Cpu size={22}/><strong>Connect a model</strong><span>Use a remote API or local server</span><ArrowRight size={18}/></button></div>
        {#if homeRooms.length || homeOpenTasks.length}
          <div class="home-work-grid">
            <section aria-labelledby="home-rooms-title" class="home-work-panel">
              <div class="home-work-heading"><h2 id="home-rooms-title">Your rooms</h2><button type="button" class="text-button" onclick={() => navigate('rooms')}>All rooms <ArrowRight size={14}/></button></div>
              {#each homeRooms as recentRoom (recentRoom.id)}
                <button type="button" class="home-work-link" onclick={() => openCommandRoom(recentRoom.id)}><span class="room-symbol">#</span><strong>{recentRoom.name}</strong><ChevronRight size={15}/></button>
              {:else}<p>No rooms created yet.</p>{/each}
            </section>
            <section aria-labelledby="home-tasks-title" class="home-work-panel">
              <div class="home-work-heading"><h2 id="home-tasks-title">Open tasks</h2><button type="button" class="text-button" onclick={() => navigate('tasks')}>All tasks <ArrowRight size={14}/></button></div>
              {#each homeOpenTasks as activeTask (activeTask.id)}
                <button type="button" class="home-work-link" onclick={() => { taskFilterRoomId = activeTask.room_id; taskSearch = activeTask.instructions.slice(0, 40); taskStatusFilter = 'all'; taskPage = 0; navigate('tasks'); }}>
                  <span class="home-task-state" data-state={activeTask.state}>{activeTask.state}</span><strong>{activeTask.instructions}</strong><ChevronRight size={15}/>
                </button>
              {:else}<p>No tasks currently awaiting action. Create a task from any room.</p>{/each}
            </section>
          </div>
        {/if}
        <div class="home-status"><div><div class="local-dot"></div> Your workspace stays on this computer by default</div><span>{snapshot.rooms.length} rooms · {snapshot.agents.length} agents · {snapshot.providers.length} providers</span></div>
      </div>
    {:else if screen === 'tasks'}
      <header class="topbar"><div class="crumb"><strong>Tasks</strong></div><button class="subtle-cta" onclick={() => openTaskDialog()}><Plus size={16}/> Plan task</button></header>
      <div class="content-page"><div class="eyebrow">WORK, WITHOUT THE GUESSWORK</div><h1>Every step,<br /><em>in the open.</em></h1><p class="page-lede">Plan work for your agents, then explicitly run a text-only task through a model you choose. Results are recorded in the room. The file tool accepts manual actions or bounded model proposals inside the app's private artifacts folder; both require independent native approval.</p>
        <div class="task-view-switch" role="group" aria-label="Tasks and workflows">
          <button type="button" class:chosen={tasksView === 'tasks'} aria-pressed={tasksView === 'tasks'} onclick={() => (tasksView = 'tasks')}>Tasks</button>
          <button type="button" class:chosen={tasksView === 'workflows'} aria-pressed={tasksView === 'workflows'} onclick={showWorkflows}>Workflows</button>
        </div>
        {#if tasksView === 'tasks'}
        <p class="task-provider-notice" id="task-run-effects">Each task has its own model selection. Review its instructions and provider before Run; the model request may use paid credits. Models cannot execute device tools. Cancelling later cannot recall an already-sent request.</p>
        <div class="tasks-filter"><label for="tasks-room-filter">Show tasks for</label><select id="tasks-room-filter" bind:value={taskFilterRoomId} onchange={() => (taskPage = 0)}><option value="">All rooms</option>{#each snapshot.rooms as taskRoom (taskRoom.id)}<option value={taskRoom.id}>{taskRoom.name}</option>{/each}</select><label for="tasks-search">Find a task</label><input id="tasks-search" type="search" bind:value={taskSearch} oninput={() => (taskPage = 0)} placeholder="Instructions or agent…" autocomplete="off" /></div>
        <div class="task-status-filter" role="group" aria-label="Filter tasks by status">
          {#each [{id:'all',label:'All'},{id:'open',label:'In progress'},{id:'done',label:'Completed'},{id:'attention',label:'Needs attention'}] as filter}
            <button type="button" class:chosen={taskStatusFilter === filter.id} aria-pressed={taskStatusFilter === filter.id} onclick={() => { taskStatusFilter = filter.id as typeof taskStatusFilter; taskPage = 0; }}>{filter.label}</button>
          {/each}
        </div>
        <div class="task-overview" aria-label="Task plan progress">
          <div><strong>{taskCounts.total}</strong><span>All tasks</span></div>
          <div><strong>{taskCounts.awaiting}</strong><span>Awaiting action</span></div>
          <div><strong>{taskCounts.running}</strong><span>Running</span></div>
          <div><strong>{taskCounts.succeeded}</strong><span>Succeeded</span></div>
          <div><strong>{taskCounts.incomplete}</strong><span>Failed or cancelled</span></div>
        </div>
        {#if roomTasks.length === 0}
          <div class="blank-panel"><div class="blank-icon"><Layers3 size={26}/></div><h2>{taskFilterRoomId ? 'No tasks in this room' : 'No tasks planned yet'}</h2><p>Create a room and a teammate, then save a task. Run text-only work manually through your chosen model; device tools are not enabled.</p><button class="soft-button" onclick={() => openTaskDialog()}>Plan your first task <ArrowRight size={15}/></button></div>
        {:else if visibleRoomTasks.length === 0}
          <div class="blank-panel"><div class="blank-icon"><Search size={26}/></div><h2>No matching tasks</h2><p>Try another status, room, or search term. Your tasks have not been changed.</p><button class="soft-button" onclick={() => { taskSearch = ''; taskStatusFilter = 'all'; taskFilterRoomId = ''; taskPage = 0; }}>Clear task filters</button></div>
        {:else}
          <p class="task-filter-count" role="status">Showing {currentTaskPage * taskPageSize + 1}–{currentTaskPage * taskPageSize + pagedRoomTasks.length} of {visibleRoomTasks.length} matching tasks ({roomTasks.length} total)</p>
          <div class="task-list" aria-label="Saved tasks">
            {#each pagedRoomTasks as task (task.id)}
              <article class="task-card">
                <div class="task-card-top"><span class="task-state" data-state={task.state}>{task.state === 'pending' ? 'Pending · Not executed' : task.state === 'cancelled' ? 'Cancelled' : task.state === 'running' ? 'Running · Text request' : task.state}</span><span class="task-date">{timeDisplay(task.created_at)}</span></div>
                <p class="task-instructions">{task.instructions}</p>
                <div class="task-card-bottom"><span>{snapshot.agents.find((agent) => agent.id === task.agent_id)?.name || 'Unknown agent'} · {snapshot.rooms.find((room) => room.id === task.room_id)?.name || 'Unknown room'}</span>
                  {#if task.state === 'succeeded'}
                    <button class="soft-button small" onclick={() => { selectRoom(task.room_id); navigate('rooms'); }}>View room result <ArrowRight size={14}/></button>
                  {/if}
                  {#if task.state === 'pending'}
                    <label class="task-card-model">Model for task execution
                      <select aria-label={'Model for task execution: ' + task.instructions.slice(0, 50)} value={taskProviders[task.id] ?? ''} disabled={!isNative || !!runningTaskId} onchange={(event) => { taskProviders = { ...taskProviders, [task.id]: event.currentTarget.value }; }}>
                        <option value="">Choose a provider for this task…</option>
                        <option value="each-agent">Use this agent’s saved model</option>
                        {#each snapshot.providers as provider (provider.id)}<option value={provider.id}>{provider.name} · {provider.model} ({providerKindLabel(provider.kind)})</option>{/each}
                      </select>
                    </label>
                    {#if taskProviderId(task)}
                      <span class="task-provider-confirmation">Ready to contact {snapshot.providers.find((provider) => provider.id === taskProviderId(task))?.name} · {snapshot.providers.find((provider) => provider.id === taskProviderId(task))?.model}. Only this task’s instructions will be sent.</span>
                    {:else}<span class="task-provider-confirmation">Choose a configured provider to enable Run. No request has been made.</span>{/if}
                    <button class="primary-button small" aria-describedby="task-run-effects" title={!taskProviderId(task) ? 'Choose a configured model above before running' : 'May send a billable model request'} disabled={!isNative || working || !!runningTaskId || !taskProviderId(task) || (!!multiStepStates[task.id] && multiStepStates[task.id].status !== 'not_started')} onclick={() => void runTask(task.id)}>{runningTaskId === task.id ? 'Running…' : 'Run text task'}</button>
                    <details class="task-extra">
                      <summary>Optional file actions · approval required</summary>
                      <div class="task-extra-actions">
                        <button type="button" class="soft-button small" aria-expanded={fileToolTaskId === task.id} disabled={!isNative || fileToolBusy || working} onclick={() => void toggleFileTool(task.id)}>{fileToolTaskId === task.id ? 'Close file actions' : 'Review a local file action'}</button>
                        <button type="button" class="soft-button small" disabled={!isNative || fileToolBusy || !!fileToolPreview || working || !!runningTaskId || !taskProviderId(task) || (!!multiStepStates[task.id] && multiStepStates[task.id].status !== 'not_started') || (fileProposalUsage?.taskId === task.id && fileProposalUsage.providerId === taskProviderId(task) && (fileProposalUsage.task_used >= fileProposalUsage.task_limit || fileProposalUsage.provider_used >= fileProposalUsage.provider_limit))} onclick={() => void requestModelFileProposal(task)}>Ask model for a file proposal · may use credits</button>
                        <button type="button" class="soft-button small" aria-controls={'recovery-evidence-' + task.id}
                          aria-expanded={!!recoveryInspections[task.id] || !!recoveryInspectionErrors[task.id] || recoveryInspectionTaskId === task.id}
                          disabled={!isNative || !!recoveryInspectionTaskId || working || !!runningTaskId}
                          onclick={() => void inspectModelRecovery(task)}>{recoveryInspectionTaskId === task.id ? 'Inspecting read-only audit…' : recoveryInspections[task.id] || recoveryInspectionErrors[task.id] ? 'Refresh recovery evidence' : 'Inspect model action recovery'}</button>
                      </div>
                      <p>Proposals never execute on their own. Every file operation still requires the exact independent native permission review.</p>
                      <section id={'recovery-evidence-' + task.id} class="recovery-evidence" aria-label="Model action recovery evidence" aria-busy={recoveryInspectionTaskId === task.id}>
                        {#if recoveryInspectionTaskId === task.id}
                          <p class="recovery-reading" role="status">Checking authenticated local history. This does not approve, retry or execute any action.</p>
                        {:else if recoveryInspectionErrors[task.id]}
                          <p class="recovery-error" role="alert">{recoveryInspectionErrors[task.id]}</p>
                        {:else if recoveryInspections[task.id]}
                          <div class="recovery-result" aria-label="Authenticated model operation recovery">
                            <strong>Read-only operation history</strong>
                            <p>No former native approval is restored. No automatic resume or replay is allowed. These observations are not instructions to the model.</p>
                            {#if recoveryInspections[task.id].operations.length === 0}
                              <p class="recovery-empty" role="status">No model-origin file actions are recorded in the authenticated turn for this pending task. This is not proof that unrelated files were never changed.</p>
                            {:else}
                              <ol class="recovery-history" aria-label="Recorded model actions">
                                {#each recoveryInspections[task.id].operations as operation (operation.operation_id)}
                                  <li class:recovery-uncertain={operation.status === 'uncertain' || operation.status === 'not_in_journal'}>
                                    <div class="recovery-heading"><strong>{recoveryStatusLabel(operation.status)}</strong><span>Checkpoint {operation.checkpoint_revision}</span></div>
                                    <p>{recoveryStatusExplanation(operation.status)}</p>
                                    {#if operation.status === 'completed'}<p><strong>Recorded outcome:</strong> {operation.outcome?.replaceAll('_', ' ')}</p>{/if}
                                    <code title="Native operation identifier">{operation.operation_id}</code>
                                  </li>
                                {/each}
                              </ol>
                              {#if recoveryInspections[task.id].operations.some((item) => item.status === 'completed' && item.outcome !== 'cancelled')}
                                <p>A completed native file action was recorded. The original provider is recovered from a signed, unchanged configuration, never from the currently selected agent provider. The native backend independently reauthenticates the result, task and one-use checkpoint before any billable request; it will not repeat an action or restore approval.</p>
                                {#if recoveryInspections[task.id].original_provider_id === null}
                                  <p class="recovery-error" role="status">No unchanged, authenticated original provider is available. A model follow-up cannot safely be requested for this recorded action.</p>
                                {:else}
                                  <p>Original provider: {snapshot.providers.find((provider) => provider.id === recoveryInspections[task.id].original_provider_id)?.name ?? 'Not configured'}</p>
                                {/if}
                                <button type="button" class="soft-button small" disabled={!isNative || working || fileToolBusy || !!runningTaskId || !recoveryInspections[task.id].original_provider_id} onclick={() => void continueAfterRecoveredTool(task)}>Request authenticated final response · one use</button>
                              {/if}
                            {/if}
                          </div>
                        {/if}
                        {#if finalFollowupError}<p class="evidence-error" role="alert">{finalFollowupError}</p>{/if}
                      </section>
                    </details>
                    <details class="task-extra" aria-label="Guided multi-step execution">
                      <summary>Guided multi-step work · separate approval for every tool</summary>
                      <p>Each click requests at most one model response. A new file action is only proposed, not executed. You must independently review every action in an OS-owned confirmation window; reads require a separate first permission to inspect. No process, Git, browser, or network tool is granted. Requests may incur provider charges, including failures.</p>
                      <div class="task-extra-actions" role="group" aria-label="Maximum model requests and file actions for this task">
                        <label for={'multi-step-model-budget-' + task.id}>Maximum provider attempts
                          <select id={'multi-step-model-budget-' + task.id} bind:value={multiStepMaxModelRequests} disabled={!!multiStepBusyId || (!!multiStepStates[task.id] && multiStepStates[task.id].status !== 'not_started')}>
                            <option value={2}>2</option><option value={4}>4</option><option value={6}>6</option>
                          </select>
                        </label>
                        <label for={'multi-step-tool-budget-' + task.id}>Maximum file actions
                          <select id={'multi-step-tool-budget-' + task.id} bind:value={multiStepMaxToolActions} disabled={!!multiStepBusyId || (!!multiStepStates[task.id] && multiStepStates[task.id].status !== 'not_started')}>
                            <option value={1}>1</option><option value={2}>2</option><option value={3}>3</option><option value={4}>4</option>
                          </select>
                        </label>
                      </div>
                      <div class="task-extra-actions">
                        <button type="button" class="soft-button small" disabled={!isNative || !taskProviderId(task) || !!multiStepBusyId || working || !!runningTaskId} onclick={() => void inspectMultiStep(task)}>{multiStepBusyId === task.id ? 'Checking authenticated state…' : 'Inspect step status · read-only'}</button>
                        {#if !multiStepStates[task.id] || ['not_started', 'awaiting_model'].includes(multiStepStates[task.id].status)}
                          <button type="button" class="primary-button small" disabled={!isNative || !taskProviderId(task) || !!multiStepBusyId || fileToolBusy || working || !!runningTaskId || !!fileToolPreview || (multiStepStates[task.id]?.model_reservations ?? 0) >= (multiStepStates[task.id]?.model_limit ?? multiStepMaxModelRequests)} onclick={() => void requestNextMultiStep(task)}>Request one next model step · may incur charges</button>
                        {/if}
                        {#if multiStepStates[task.id]?.status === 'awaiting_native_review'}
                          <button type="button" class="primary-button small" disabled={!!multiStepBusyId || fileToolBusy || working || !!runningTaskId} onclick={() => void resolveMultiStep(task, 'review')}>Review exact operation in native OS dialog</button>
                          <button type="button" class="soft-button small" disabled={!!multiStepBusyId || fileToolBusy || working || !!runningTaskId} onclick={() => void resolveMultiStep(task, 'deny')}>Deny this operation</button>
                        {/if}
                      </div>
                      {#if multiStepStates[task.id] && multiStepStates[task.id].status !== 'not_started'}
                        {#if multiStepStates[task.id].status === 'quarantined'}
                          <p role="status">Authenticated step: <strong>quarantined</strong>. Budget counters and previous tool outcomes are not independently recoverable here. No model request or native action may continue from this checkpoint.</p>
                        {:else}
                          <p role="status">Authenticated step: <strong>{multiStepStates[task.id].status.replaceAll('_', ' ')}</strong>. {multiStepStates[task.id].model_reservations ?? 'Unknown'} of {multiStepStates[task.id].model_limit ?? 'Unknown'} provider attempts; {multiStepStates[task.id].tool_steps ?? 'Unknown'} of {multiStepStates[task.id].tool_limit ?? 'Unknown'} proposed tools.</p>
                        {/if}
                        {#if multiStepStates[task.id].status === 'awaiting_native_review'}
                          <p>Model-proposed {multiStepStates[task.id].action === 'read' ? 'file read' : 'new file creation'}: <code>{multiStepStates[task.id].relative_path}</code>. This is a relative path, not proof of permission. Review the complete immutable path and content/digest in the independent native confirmation before deciding.</p>
                        {/if}
                        {#if multiStepStates[task.id].status === 'quarantined'}
                          <p class="recovery-error" role="status">An interrupted, stale or mismatched checkpoint cannot be resumed. Previously proposed or uncertain operations must never be replayed.</p>
                        {/if}
                      {/if}
                      {#if multiStepNotices[task.id]}<p role="status">{multiStepNotices[task.id]}</p>{/if}
                      {#if multiStepErrors[task.id]}<p class="evidence-error" role="alert">{multiStepErrors[task.id]}</p>{/if}
                    </details>
                    {#if fileProposalUsage?.taskId === task.id && fileProposalUsage.providerId === taskProviderId(task)}
                      <span class="task-date" aria-label="Durable model request limits">Proposal requests: {fileProposalUsage.task_used}/{fileProposalUsage.task_limit} for this task; {fileProposalUsage.provider_used}/{fileProposalUsage.provider_limit} for this provider. Failed and cancelled requests also count.</span>
                    {/if}
                  {/if}
                  {#if task.state === 'pending' || task.state === 'queued' || task.state === 'running'}
                    <button type="button" class="soft-button small" disabled={!isNative || working} aria-expanded={taskCancelConfirmation === task.id} onclick={() => void cancelTask(task.id)}>{taskCancelConfirmation === task.id ? 'Confirm cancel task' : 'Cancel task'}</button>
                    {#if taskCancelConfirmation === task.id}
                      <div class="inline-confirmation" role="group" aria-label="Confirm task cancellation">
                        <span>Cancel this task? The remote model may already have received a request.</span>
                        <button type="button" class="soft-button small" onclick={() => taskCancelConfirmation = ''}>Keep task</button>
                      </div>
                    {/if}
                  {/if}
                </div>
                {#if fileToolTaskId === task.id && task.state === 'pending'}
                  <section class="task-file-tools" aria-label="Approved local file operations">
                    <h3>Local file action · requires your approval</h3>
                    <p>Use files in this app's private artifacts folder. You can prepare a manual operation or ask your selected model to suggest one strict JSON file operation. Model output cannot grant permission or execute itself. Every read requires two separate native decisions: first permission to inspect the named path, then permission to use the displayed exact SHA-256 and byte count. Create-new actions require one native approval. Each approval applies only once.</p>
                    {#if fileProposalText}<p class="file-tool-result" role="status"><strong>Model completed without a file action</strong><span>{fileProposalText}</span><span>Its final answer was saved with the completed task. No local file operation was prepared or executed.</span></p>{/if}
                    <div class="file-tool-choice" role="group" aria-label="File action type">
                      <button type="button" aria-pressed={fileToolAction === 'create_new'} disabled={fileToolBusy || !!fileToolPreview} onclick={() => { fileToolAction = 'create_new'; clearFileToolDraft(); }}>Create a new text file</button>
                      <button type="button" aria-pressed={fileToolAction === 'read'} disabled={fileToolBusy || !!fileToolPreview} onclick={() => { fileToolAction = 'read'; clearFileToolDraft(); }}>Read an existing text file</button>
                    </div>
                    <form class="file-tool-form" onsubmit={(event) => { event.preventDefault(); void prepareFileTool(); }}>
                      <label for="file-tool-path">Relative filename inside private artifacts folder</label>
                      <input id="file-tool-path" type="text" autocomplete="off" spellcheck="false" bind:value={fileToolPath} maxlength="1024" placeholder="notes/example.md" disabled={fileToolBusy || !!fileToolPreview} required />
                      {#if fileToolAction === 'create_new'}
                        <label for="file-tool-content">File content (up to 16 KiB)</label>
                        <textarea id="file-tool-content" bind:value={fileToolContent} spellcheck="false" rows="5" maxlength="16384" disabled={fileToolBusy || !!fileToolPreview} required></textarea>
                        <p>{utf8Bytes(fileToolContent)} / 16384 UTF-8 bytes. Existing files will never be replaced.</p>
                      {/if}
                      <button type="submit" class="soft-button small" disabled={!isNative || fileToolBusy || !!fileToolPreview || !fileToolPath.trim() || (fileToolAction === 'create_new' && (!fileToolContent || utf8Bytes(fileToolContent) > 16384))}>{fileToolAction === 'read' ? 'Request native permission to inspect' : 'Preview exact file action'}</button>
                    </form>
                    {#if fileToolPreview}
                      <div class="file-tool-review" role="group" aria-label="Approval required for proposed file action">
                        <strong>{fileToolPreview.origin === 'model_proposal' ? 'Model-proposed action — review before allowing' : 'Review before allowing this one operation'}</strong>
                        <span>{fileToolPreview.action === 'read' ? 'Read text' : 'Create new text file'} · task {task.id}</span>
                        <code>{fileToolPreview.canonical_path}</code>
                        {#if fileToolPreview.action === 'create_new' && fileToolPreview.byte_count !== undefined}<span>{fileToolPreview.byte_count} bytes · SHA-256 {fileToolPreview.sha256}</span>{/if}
                        {#if fileToolPreview.action === 'create_new' && fileToolPreview.reviewed_content !== undefined}
                          <pre>{fileToolPreview.reviewed_content}</pre>
                        {/if}
                        <p>Access is currently denied. {fileToolPreview.action === 'read' ? 'The file size and SHA-256 are private until the independent native OS consent dialog. Review the exact SHA-256 in that dialog before choosing Yes. File contents are not previewed here; changed bytes are rejected.' : 'Review the full proposed file content before granting create access.'} A rejected or consumed operation cannot be reused.</p>
                        <div class="file-tool-approval-actions">
                          <button type="button" class="primary-button small" disabled={fileToolBusy} onclick={() => void approveFileTool()}>Approve this exact file action</button>
                          <button type="button" class="soft-button small" disabled={fileToolBusy} onclick={() => void rejectFileTool()}>Reject without executing</button>
                        </div>
                      </div>
                    {/if}
                    {#if fileToolResult}
                      <div class="file-tool-result" role="status">
                        <strong>Audit result: {fileToolResult.status}</strong>
                        <span>{fileToolResult.bytes} bytes · SHA-256 {fileToolResult.sha256 ?? 'not available'}</span>
                        {#if fileToolResult.content !== null}<pre>{fileToolResult.content}</pre>{/if}
                        {#if fileToolResult.error}<p>{fileToolResult.error}</p>{/if}
                        <span>This operation ID is consumed. No file bytes or private path were sent to a model.</span>
                        {#if fileToolResult.model_observation_authenticated === true && fileToolOriginProviderId && task.state === 'pending'}
                          <p>The independent native audit confirms this model-origin operation. You can request <strong>one final text-only response</strong> from the original provider. This may use paid credits. Only the fixed outcome metadata is shared, not file contents. The request is consumed even if the provider fails or you cancel it; no additional device action is allowed.</p>
                          <button type="button" class="primary-button small" disabled={fileToolBusy || working || !!runningTaskId} onclick={() => void continueAfterReviewedTool(task)}>Request final answer · one model call</button>
                        {:else if fileToolResult.model_observation_authenticated === false}
                          <p>The model-origin result could not be independently authenticated for continuation. No further provider request is available from this receipt.</p>
                        {/if}
                      </div>
                    {/if}
                    {#if finalFollowupError}<p class="evidence-error" role="alert">{finalFollowupError}</p>{/if}
                    {#if fileToolError}<p class="evidence-error" role="alert">{fileToolError}</p>{/if}
                  </section>
                {/if}
                {#if task.failure_reason}<p class="task-failure" role="status">Task failure: {task.failure_reason}</p>{/if}
              </article>
            {/each}
          </div>
          {#if visibleRoomTasks.length > taskPageSize}
            <nav class="task-pages" aria-label="Task result pages">
              <button type="button" class="soft-button small" disabled={currentTaskPage === 0 || !!runningTaskId || working} onclick={() => (taskPage = Math.max(0, currentTaskPage - 1))}>Previous tasks</button>
              <span role="status" aria-live="polite">Page {currentTaskPage + 1} of {taskLastPage + 1}</span>
              <button type="button" class="soft-button small" disabled={currentTaskPage === taskLastPage || !!runningTaskId || working} onclick={() => (taskPage = Math.min(taskLastPage, currentTaskPage + 1))}>Next tasks</button>
            </nav>
          {/if}
        {/if}
        {:else}
          <section class="workflow-workspace" aria-label="Local workflow plans">
            <h2>Plan a small team workflow</h2>
            <p class="task-provider-notice">Create a research → writing → review sequence with one to three steps. Each step needs an existing agent and an explicitly selected model. Previewing or saving a plan does not contact a provider, run AI or execute device tools.</p>
            <ol class="workflow-progress" aria-label="Workflow steps">
              <li>Choose agents and models</li><li>Preview locally</li><li>Save, then manually run</li>
            </ol>
            <form class="workflow-builder" onsubmit={(event) => { event.preventDefault(); void previewWorkflow(); }}>
              <div class="workflow-build-header">
                <label for="workflow-goal">What should the team accomplish?
                  <textarea id="workflow-goal" bind:value={workflowBrief} oninput={invalidateWorkflowPreview} rows="3" maxlength="1500" disabled={workflowBusy} placeholder="Describe the research question or deliverable…" required></textarea>
                </label>
                <label for="workflow-step-count">Steps
                  <select id="workflow-step-count" value={workflowStepCount} disabled={workflowBusy} onchange={(event) => { workflowStepCount = Number(event.currentTarget.value); invalidateWorkflowPreview(); }}>
                    <option value="1">Research only</option>
                    <option value="2">Research → Write</option>
                    <option value="3">Research → Write → Review</option>
                  </select>
                </label>
              </div>
              <div class="workflow-steps">
                {#each activeWorkflowSteps as step, index (step.id)}
                  <fieldset class="workflow-step">
                    <legend>{index + 1}. {step.title}</legend>
                    <p>{index === 0 ? 'Ready first' : `After ${activeWorkflowSteps[index - 1].title.toLowerCase()} succeeds`}. Planning only; no automatic dispatch.</p>
                    <div class="workflow-step-selects">
                      <label for={'workflow-agent-' + step.id}>Assigned agent
                        <select id={'workflow-agent-' + step.id} bind:value={workflowSteps[index].agentId} onchange={invalidateWorkflowPreview} disabled={workflowBusy} required>
                          <option value="">Choose an agent…</option>
                          {#each snapshot.agents as agent (agent.id)}<option value={agent.id}>{agent.name} · {agent.role}</option>{/each}
                        </select>
                      </label>
                      <label for={'workflow-provider-' + step.id}>Model provider
                        <select id={'workflow-provider-' + step.id} bind:value={workflowSteps[index].providerId} onchange={invalidateWorkflowPreview} disabled={workflowBusy} required>
                          <option value="">Choose explicitly…</option>
                          {#each snapshot.providers as provider (provider.id)}<option value={provider.id}>{provider.name} · {provider.model}</option>{/each}
                        </select>
                      </label>
                    </div>
                    <details class="workflow-button-picker">
                      <summary>Choose an agent or model using buttons</summary>
                      <div class="workflow-button-picker-groups">
                        <div role="group" aria-label={'Agent choices for ' + step.title}>
                          <strong>Assigned agent</strong>
                          {#each snapshot.agents as agent (agent.id)}
                            <button type="button" aria-pressed={workflowSteps[index].agentId === agent.id} disabled={workflowBusy} onclick={() => { workflowSteps[index].agentId = agent.id; invalidateWorkflowPreview(); }}>{agent.name} · {agent.role}</button>
                          {:else}
                            <span>Add an agent first.</span>
                          {/each}
                        </div>
                        <div role="group" aria-label={'Model choices for ' + step.title}>
                          <strong>Model provider</strong>
                          {#each snapshot.providers as provider (provider.id)}
                            <button type="button" aria-pressed={workflowSteps[index].providerId === provider.id} disabled={workflowBusy} onclick={() => { workflowSteps[index].providerId = provider.id; invalidateWorkflowPreview(); }}>{provider.name} · {provider.model}</button>
                          {:else}
                            <span>Connect a model first.</span>
                          {/each}
                        </div>
                      </div>
                    </details>
                    {#if step.agentId}<p class="workflow-role">Role: {snapshot.agents.find((agent) => agent.id === step.agentId)?.role ?? 'Agent missing'}</p>{/if}
                    <label for={'workflow-instructions-' + step.id}>Instructions for {step.title.toLowerCase()}
                      <textarea id={'workflow-instructions-' + step.id} bind:value={workflowSteps[index].instruction} oninput={invalidateWorkflowPreview} maxlength="2300" rows="2" disabled={workflowBusy} required></textarea>
                    </label>
                  </fieldset>
                {/each}
              </div>
              {#if snapshot.agents.length === 0 || snapshot.providers.length === 0}
                <p class="workflow-help">Add agents from the Role library and model profiles from Settings before building a workflow.</p>
              {/if}
              <div class="workflow-actions">
                <button type="submit" class="soft-button" disabled={!isNative || workflowBusy || !workflowBrief.trim() || !activeWorkflowSteps.every((step) => step.agentId && step.providerId && step.instruction.trim())}>{workflowBusy ? 'Validating…' : 'Preview dependencies'}</button>
                <button type="button" class="primary-button" disabled={!isNative || workflowBusy || !workflowPreviewCurrent} onclick={() => void saveWorkflow()}>Save local plan</button>
              </div>
              {#if workflowPreviewCurrent}<div class="workflow-preview-result" role="status"><strong>Valid plan · {workflowPreview!.step_count} steps</strong><span>Order: {workflowPreview!.ordered_step_ids.join(' → ')}</span><span>Ready: {workflowPreview!.ready_step_ids.join(', ')}</span><p>Preview only. Save to store the plan. No agent will run automatically.</p></div>{/if}
              {#if workflowNotice}<p class="workflow-feedback" role="status">{workflowNotice}</p>{/if}
              {#if workflowError}<p class="evidence-error" role="alert">{workflowError}</p>{/if}
            </form>
            <section class="workflow-saved" aria-label="Saved local workflows">
              <div class="workflow-saved-header"><h2>Saved workflows</h2><button type="button" class="soft-button small" disabled={!isNative || workflowLoading || workflowBusy} onclick={() => void loadWorkflows()}>{workflowLoading ? 'Refreshing…' : 'Refresh list'}</button></div>
              <form class="workflow-search" aria-label="Find saved workflow by ID" onsubmit={(event) => { event.preventDefault(); void loadWorkflows(0, workflowSearchQuery.trim()); }}>
                <label for="workflow-id-search">Find workflow ID</label>
                <input id="workflow-id-search" type="search" bind:value={workflowSearchQuery} maxlength="120" placeholder="Search all saved workflow IDs…" autocomplete="off" aria-describedby="workflow-id-search-help" disabled={!isNative || workflowLoading || workflowBusy || workflowCancelBusy} />
                <button type="submit" class="soft-button small" disabled={!isNative || workflowLoading || workflowBusy || workflowCancelBusy}>Search</button>
                {#if workflowAppliedSearch}<button type="button" class="soft-button small" disabled={workflowLoading || workflowBusy || workflowCancelBusy} onclick={() => { workflowSearchQuery = ''; void loadWorkflows(0, ''); }}>Clear filter</button>{/if}
              </form>
              <p id="workflow-id-search-help" class="workflow-help">Finds matching workflow IDs across all saved pages, entirely on this device. Search is literal, not a wildcard; 128 UTF-8 bytes maximum.</p>
              {#if workflowNotice}<p class="workflow-feedback" role="status" aria-live="polite">{workflowNotice}</p>{/if}
              {#if workflowError}<p class="evidence-error" role="alert">{workflowError}</p>{/if}
              {#if !workflowLoaded}<p class="workflow-help">Open the desktop app and refresh to see locally saved plans.</p>
              {:else if savedWorkflows.length === 0}
                <p class="workflow-help">{workflowAppliedSearch ? 'No saved workflows match that ID search.' : workflowOffset === 0 ? 'No workflows saved on this device yet.' : 'No workflows on this page. The list may have changed in another window.'}</p>
                {#if workflowOffset > 0}<button type="button" class="soft-button small" disabled={workflowLoading} onclick={() => void loadWorkflows(0)}>Return to first page</button>{/if}
              {:else}
                <div class="workflow-saved-list">
                  {#each savedWorkflows as item (item.definition.workflow_id)}
                    <article class="workflow-saved-item">
                      <div class="workflow-saved-heading"><strong>{item.definition.workflow_id}</strong><span class="task-state" data-state={item.cancelled ? 'cancelled' : 'pending'}>{item.cancelled ? 'Cancelled' : item.definition.steps.every((step) => item.states?.[step.id]?.status === 'succeeded') ? 'Completed' : item.definition.steps.some((step) => item.states?.[step.id]?.status === 'succeeded') ? 'Partly completed' : 'Saved plan'}</span></div>
                      <p>{item.definition.steps.length} step{item.definition.steps.length === 1 ? '' : 's'} · Revision {item.revision} · {item.ready_step_ids.length ? `Ready: ${item.ready_step_ids.join(', ')}` : 'No ready steps'}</p>
                      {#if !item.cancelled && item.ready_step_ids.length > 0 && item.definition.steps.length <= 3}
                        <button type="button" class="primary-button" disabled={workflowBusy || workflowLoading} title="Sends text to the selected providers; may use paid credits" onclick={() => void runWorkflow(item)}>Run remaining text steps · up to 3 provider calls</button>
                      {/if}
                      <details class="workflow-more-actions">
                        <summary>Review steps, provider results and export</summary>
                        <p>Reports describe locally saved results; they are not independent proof that a model's claims are correct. Copying exports text to your system clipboard.</p>
                        <div class="workflow-actions">
                          <button type="button" class="soft-button small" disabled={workflowBusy} onclick={() => void copyWorkflowReport(item)}>Copy workflow report · Markdown</button>
                          <button type="button" class="soft-button small" disabled={workflowBusy} aria-expanded={displayedWorkflowReportId === item.definition.workflow_id} onclick={() => void viewWorkflowReport(item)}>{displayedWorkflowReportId === item.definition.workflow_id ? 'Hide Markdown report' : 'View Markdown report'}</button>
                        </div>
                      {#if displayedWorkflowReportId === item.definition.workflow_id}
                        <section class="workflow-result" aria-label={'Saved Markdown report for ' + item.definition.workflow_id}>
                          <pre>{displayedWorkflowMarkdown}</pre>
                        </section>
                      {/if}
                      <ol>{#each item.definition.steps as step (step.id)}<li><strong>{step.id}</strong> · {snapshot.agents.find((agent) => agent.id === step.agent_id)?.name ?? 'Unavailable agent'} · {snapshot.providers.find((provider) => provider.id === step.provider_id)?.name ?? 'Unavailable provider'} · {item.states?.[step.id]?.status ?? 'Unknown state'}
                        {#if !item.cancelled && item.ready_step_ids.includes(step.id)}
                          <button type="button" class="soft-button small" disabled={workflowBusy || workflowLoading} onclick={() => void runWorkflowStep(item, step.id)}>Run text step · may use provider credits</button>
                        {/if}
                        {#if !item.cancelled && item.states?.[step.id]?.status === 'awaiting_retry'}
                          <button type="button" class="soft-button small" disabled={workflowBusy || workflowLoading} onclick={() => void retryWorkflowStep(item, step.id)}>Allow retry</button>
                        {/if}
                        {#if item.states?.[step.id]?.result_text}
                          <details class="workflow-result"><summary>View saved result for {step.id}</summary><pre>{item.states[step.id].result_text}</pre></details>
                        {/if}
                        {#if item.states?.[step.id]?.status === 'blocked'}<p class="workflow-help">An earlier attempt was interrupted. Cancel this plan or create a new plan after reviewing what happened.</p>{/if}
                      </li>{/each}</ol>
                      </details>
                      {#if !item.cancelled}
                        <button type="button" class="soft-button small" aria-controls={'workflow-cancel-' + item.definition.workflow_id} disabled={!isNative || workflowCancelBusy || workflowLoading} onclick={() => void cancelWorkflow(item)}>{workflowCancelConfirmation === item.definition.workflow_id ? 'Confirm cancel pending steps' : 'Cancel pending steps'}</button>
                        {#if workflowCancelConfirmation === item.definition.workflow_id}
                          <div id={'workflow-cancel-' + item.definition.workflow_id} class="inline-confirmation" role="group" aria-label="Confirm workflow cancellation">
                            <span>Stop all pending steps? Already-sent provider requests cannot be recalled.</span>
                            <button type="button" class="soft-button small" onclick={() => workflowCancelConfirmation = ''}>Keep workflow</button>
                          </div>
                        {/if}
                      {/if}
                    </article>
                  {/each}
                </div>
              {/if}
              {#if workflowLoaded && (workflowOffset > 0 || workflowHasMore)}
                <nav class="workflow-pages" aria-label="Saved workflow pages">
                  <button type="button" class="soft-button small" disabled={!isNative || workflowLoading || workflowBusy || workflowCancelBusy || workflowOffset === 0} onclick={() => void loadWorkflows(Math.max(0, workflowOffset - workflowPageSize))}>Previous page</button>
                  <span role="status" aria-live="polite">{savedWorkflows.length ? `Workflows ${workflowOffset + 1}–${workflowOffset + savedWorkflows.length}` : `Page ${workflowOffset / workflowPageSize + 1}`} · Page {workflowOffset / workflowPageSize + 1}{workflowAppliedSearch ? ' · Filtered by ID' : ''}</span>
                  <button type="button" class="soft-button small" disabled={!isNative || workflowLoading || workflowBusy || workflowCancelBusy || !workflowHasMore || workflowOffset >= 100_000} onclick={() => void loadWorkflows(workflowOffset + workflowPageSize)}>Next page</button>
                </nav>
              {/if}
            </section>
          </section>
        {/if}
      </div>
    {:else if screen === 'evidence'}
      <header class="topbar"><div class="crumb"><strong>Evidence</strong></div><span class="privacy-note"><ShieldCheck size={14}/> Local only</span></header>
      <div class="content-page evidence-page">
        <div class="eyebrow">FIND THE ORIGINAL WORDS</div><h1>Search what<br /><em>you know.</em></h1>
        <p class="page-lede">Paste text or Markdown to find exact passages offline, or import an explicitly selected PDF/DOCX. An ordinary search or extraction never saves your text or contacts an AI provider. You can separately choose to save extracted text in this local project and search saved sources together.</p>
        <nav aria-label="Evidence workflow" class="evidence-journey">
          <a href="#evidence-import-section"><strong>1</strong><span>Import or paste <small>Keep it private</small></span></a>
          <a href="#evidence-draft-search"><strong>2</strong><span>Find passages <small>Exact source text</small></span></a>
          <a href="#evidence-saved-section"><strong>3</strong><span>Save or export <small>Only when you choose</small></span></a>
        </nav>
        {#if evidenceError}<p class="evidence-error evidence-page-alert" role="alert">{evidenceError}</p>{/if}
        <section class="evidence-results evidence-import" id="evidence-import-section" aria-label="Import local document text">
          <h2>1. Import a PDF or DOCX <span class="evidence-optional">Optional</span></h2>
          <p class="evidence-field-note">Choose a document up to 2 MiB to extract text entirely on this device. Some ordinary compressed or encrypted PDFs are not supported. Keep the original to independently verify page references; imported text is not automatically saved.</p>
          <label class="evidence-field-note evidence-rights"><input type="checkbox" bind:checked={evidenceImportRights} disabled={evidenceImportBusy}/> <span>I confirm I have permission to locally extract and quote this document.</span></label>
          <label for="evidence-import-file">Choose a local PDF or DOCX</label>
          <input type="file" id="evidence-import-file" aria-describedby="evidence-import-guidance" accept=".pdf,.docx,application/pdf,application/vnd.openxmlformats-officedocument.wordprocessingml.document" disabled={!isNative || !evidenceImportRights || evidenceBusy || evidenceImportBusy} onchange={(event) => { const element = event.currentTarget; const file = element.files?.[0]; element.value = ''; void importEvidenceDocument(file); }}/>
          <p class="evidence-field-note" id="evidence-import-guidance">You can skip this and paste text below. The file picker unlocks after you confirm rights.</p>
          {#if evidencePendingImport}
            <div class="inline-confirmation" role="group" aria-label="Confirm replacing current draft with imported document text">
              <span>Importing “{evidencePendingImport.name}” will replace the current text draft and clear its temporary search results. Unsaved edits will be lost. The selected file has not been read or stored yet.</span>
              <button type="button" class="primary-button small" disabled={evidenceBusy || !evidenceImportRights} onclick={() => { const selected = evidencePendingImport ?? undefined; evidencePendingImport = null; void importEvidenceDocument(selected, true); }}>Replace draft and import</button>
              <button type="button" class="soft-button small" onclick={() => evidencePendingImport = null}>Keep current draft</button>
            </div>
          {/if}
          {#if evidenceImportBusy}<p class="evidence-field-note" role="status">Extracting document text offline…</p>{/if}
          {#if evidenceImportInfo}<details class="evidence-provenance" open><summary>Import completed · show source and verification details</summary><p>{evidenceImportInfo}</p></details>{/if}
        </section>
        <form class="evidence-form" id="evidence-draft-search" aria-label="Search the current unsaved text draft" aria-busy={evidenceBusy} onsubmit={(event) => { event.preventDefault(); void searchEvidence(); }}>
          <h2>2. Search this text <span class="evidence-optional">Not saved</span></h2>
          <label for="evidence-document">Current text <span>up to 128 KiB UTF-8</span></label>
          <textarea id="evidence-document" bind:value={evidenceText} oninput={() => { forgetImportedProvenance(); evidenceLoadConfirmation = null; }} aria-describedby="evidence-document-limit evidence-document-privacy" aria-invalid={documentTooLarge} spellcheck="false" rows="9" placeholder="Paste or import source text here…" disabled={evidenceBusy}></textarea>
          <p class:over-limit={documentTooLarge} class="evidence-field-note" id="evidence-document-limit">{documentBytes.toLocaleString()} / {maxEvidenceDocumentBytes.toLocaleString()} UTF-8 bytes{documentTooLarge ? ' — shorten the document before searching.' : ''}</p>
          <label for="evidence-query">Search terms <span>up to 2,048 UTF-8 bytes</span></label>
          <div class="evidence-search-row"><input id="evidence-query" bind:value={evidenceQuery} oninput={clearEvidenceResults} aria-describedby="evidence-query-limit" aria-invalid={queryTooLarge} maxlength="2048" placeholder="e.g. hippocampus and memory…" disabled={evidenceBusy}/><button class="primary-button" type="submit" disabled={!isNative || evidenceBusy || documentTooLarge || queryTooLarge || !evidenceText.trim() || !evidenceQuery.trim()}><Search size={16}/>{evidenceBusy ? 'Searching…' : 'Find in current text'}</button></div>
          <p class:over-limit={queryTooLarge} class="evidence-field-note" id="evidence-query-limit">{queryBytes.toLocaleString()} / {maxEvidenceQueryBytes.toLocaleString()} UTF-8 bytes{queryTooLarge ? ' — shorten the search terms.' : ''}</p>
          <div class="evidence-form-footer"><p id="evidence-document-privacy">Searching here does not save this draft. Original PDF/DOCX page markers apply only until the extracted text is edited. Keep the original to verify quotations.</p><button type="button" class="soft-button small" disabled={evidenceBusy || (!evidenceText && !evidenceQuery)} onclick={() => { evidenceText = ''; evidenceQuery = ''; forgetImportedProvenance(); evidenceLoadConfirmation = null; }}>Clear current draft</button></div>
          {#if evidenceBusy}<p class="evidence-field-note" role="status">Searching the pasted document on this device…</p>{/if}
        </form>
        <section class="evidence-results evidence-library" id="evidence-saved-section" tabindex="-1" aria-label="Saved local evidence library">
          <h2>3. Saved project sources <span class="evidence-optional">Your choice</span></h2>
          <p class="evidence-field-note">Saving is separate from importing or searching. Only extracted/plain text is stored in this project database, not the original PDF/DOCX. Encrypted project archives may contain earlier saved copies. Up to 128 sources and 8 MiB of source text.</p>
          <label for="evidence-save-title">Source title</label>
          <div class="evidence-search-row">
            <input id="evidence-save-title" bind:value={evidenceSaveTitle} oninput={() => evidenceNotice = ''} maxlength="120" placeholder="e.g. Memory study notes" disabled={evidenceBusy || evidenceSaving}/>
            <button type="button" class="soft-button small" disabled={!isNative || evidenceSaving || evidenceBusy || documentTooLarge || !evidenceText.trim() || !evidenceSaveTitle.trim()} onclick={() => void saveEvidence()}>{evidenceSaving ? 'Saving…' : 'Save source'}</button>
          </div>
          <div class="evidence-form-footer">
            <button type="button" class="soft-button small" disabled={!isNative || evidenceBusy || evidenceSaving} onclick={() => void refreshSavedEvidence()}>Refresh saved sources</button>
            <button type="button" class="primary-button" aria-describedby="evidence-saved-search-description" disabled={!isNative || evidenceBusy || evidenceSaving || !evidenceQuery.trim() || queryTooLarge} onclick={() => void searchSavedEvidence()}>{evidenceBusy ? 'Searching…' : 'Find in saved sources'}</button>
            <button type="button" class="soft-button small" disabled={!isNative || evidenceBusy || evidenceSaving || !evidenceQuery.trim() || queryTooLarge} title="Copies verified source excerpts to the operating-system clipboard; no AI or network call" onclick={() => void copySavedEvidencePack()}>Copy saved excerpts · Markdown</button>
          </div>
          <p class="evidence-field-note" id="evidence-saved-search-description">Uses the same search terms as above, but searches only stored project sources. Copy exports verified quotations and source coordinates to your system clipboard, where other apps may read them.</p>
          {#if evidenceNotice}<p class="evidence-feedback" role="status" aria-live="polite">{evidenceNotice}</p>{/if}
          {#if evidenceInventoryError}<p class="evidence-error" role="alert">{evidenceInventoryError}</p>{/if}
          {#if evidenceSavedSources.length}
            <ul class="evidence-source-list" aria-label="Sources saved in this project">
              {#each evidenceSavedSources as saved (saved.source_id)}
                <li class="evidence-source-item">
                  <div class="evidence-source-overview"><strong>{saved.title}</strong><span>{saved.bytes.toLocaleString()} UTF-8 bytes · Saved locally</span></div>
                  <div class="evidence-source-actions">
                    <button type="button" class="soft-button small" disabled={evidenceBusy || evidenceSaving} aria-label={`Open saved source ${saved.title} in current draft`} aria-expanded={evidenceLoadConfirmation?.sourceId === saved.source_id && evidenceLoadConfirmation?.sha256 === saved.source_sha256} onclick={() => void loadSavedEvidence(saved)}>Open in draft</button>
                    <button type="button" class="soft-button small" disabled={evidenceBusy || evidenceSaving} aria-label={evidenceDeleteConfirmation?.sourceId === saved.source_id && evidenceDeleteConfirmation?.sha256 === saved.source_sha256 ? `Confirm removing ${saved.title}` : `Remove ${saved.title} from this project`} aria-expanded={evidenceDeleteConfirmation?.sourceId === saved.source_id && evidenceDeleteConfirmation?.sha256 === saved.source_sha256} onclick={() => void deleteSavedEvidence(saved)}>{evidenceDeleteConfirmation?.sourceId === saved.source_id && evidenceDeleteConfirmation.sha256 === saved.source_sha256 ? 'Confirm remove' : 'Remove source'}</button>
                  </div>
                  {#if evidenceLoadConfirmation?.sourceId === saved.source_id && evidenceLoadConfirmation?.sha256 === saved.source_sha256}
                    <div class="inline-confirmation" role="group" aria-label={`Confirm replacing the current draft with ${saved.title}`}>
                      <span>Opening this source replaces the text currently in the draft. Unsaved edits and temporary search results will be lost. The saved source itself is not changed.</span>
                      <button type="button" class="primary-button small" disabled={evidenceBusy || evidenceSaving} onclick={() => void loadSavedEvidence(saved)}>Replace current draft</button>
                      <button type="button" class="soft-button small" onclick={() => evidenceLoadConfirmation = null}>Keep my draft</button>
                    </div>
                  {/if}
                  {#if evidenceDeleteConfirmation?.sourceId === saved.source_id && evidenceDeleteConfirmation?.sha256 === saved.source_sha256}
                    <div class="inline-confirmation" role="group" aria-label={`Confirm deletion of ${saved.title}`}>
                      <span>This deletes the saved source from this project, not the original file. Older encrypted backups remain unchanged.</span>
                      <button type="button" class="soft-button small" onclick={() => evidenceDeleteConfirmation = null} disabled={evidenceSaving}>Keep source</button>
                    </div>
                  {/if}
                  <details class="evidence-provenance"><summary>Source identity and verification</summary><p>Local stored path: {saved.relative_path}</p><code>Source SHA-256: {saved.source_sha256}</code><p>Page and paragraph markers from original PDF/DOCX imports are not retained in saved-text results.</p></details>
                </li>
              {/each}
            </ul>
          {:else if evidenceInventoryLoaded}<p class="evidence-field-note">No saved sources in this project. You can search current text without saving, or save a source above.</p>
          {:else}<p class="evidence-field-note">Saved sources have not been loaded or verified. Select Refresh to inspect this device's project library.</p>{/if}
        </section>
        {#if evidenceSearched}
          <section class="evidence-results" aria-label="Document evidence results"><h2 role="status">{evidenceHits.length ? `${evidenceSavedResults ? `Showing ranked matches ${evidenceSavedOffset + 1}–${evidenceSavedOffset + evidenceHits.length} from saved sources` : `${evidenceHits.length} matching passages from current text`}` : 'No matching passages'}</h2>
            <p class="evidence-field-note">Matches are verbatim UTF-8 spans checked against {evidenceSavedResults ? 'the saved source text' : 'this exact current draft'}. A matching passage does not independently validate its claims. Confirm citations in the original document before relying on them.</p>
            {#each evidenceHits as hit (`${hit.source_sha256}-${hit.start_byte}-${hit.end_byte}`)}
                  <article class="evidence-hit"><div class="evidence-hit-meta"><span><FileSearch size={14}/> {hit.relative_path} · UTF-8 bytes [{hit.start_byte}, {hit.end_byte})</span><button type="button" class="soft-button small" onclick={() => void copyEvidence(hit)}>Copy quote and citation</button></div><blockquote>{hit.quote}</blockquote>{#if evidenceAnchorLabel(hit)}<p class="evidence-field-note">Extracted from {evidenceAnchorLabel(hit)}. Verify in the original file; page/paragraph mapping is not saved with extracted text.</p>{/if}<details class="evidence-provenance"><summary>Check exact source SHA-256</summary><div class="evidence-hash" role="note" aria-label={'Source SHA-256: ' + hit.source_sha256}>Source SHA-256: {hit.source_sha256}</div></details></article>
            {:else}<p class="evidence-empty">Try different terms. No passage is shown without a source match.</p>{/each}
            {#if evidenceSavedResults && (evidenceSavedOffset > 0 || evidenceSavedHasMore)}
              <nav class="evidence-pages" aria-label="Saved evidence result pages">
                <button type="button" class="soft-button small" disabled={evidenceBusy || evidenceSavedOffset === 0} onclick={() => void searchSavedEvidence(Math.max(0, evidenceSavedOffset - 10))}>Previous evidence</button>
                <span role="status" aria-live="polite">Page {evidenceSavedOffset / 10 + 1} · Top 1,000 ranked matches maximum</span>
                <button type="button" class="soft-button small" disabled={evidenceBusy || !evidenceSavedHasMore} onclick={() => void searchSavedEvidence(evidenceSavedOffset + 10)}>Next evidence</button>
              </nav>
            {/if}
          </section>
        {/if}
      </div>
    {:else if screen === 'library'}
      <header class="topbar"><div class="crumb"><strong>Role library</strong></div><button class="subtle-cta" onclick={() => (dialog = 'new-agent')}><Plus size={16}/> Create agent</button></header>
      <div class="content-page library-page">
        <div class="eyebrow">THE RIGHT TEAM FOR ANY IDEA</div><h1>Find your <em>people.</em></h1>
        <p class="page-lede">Browse curated roles. Each role provides tailored guidance when chatting with a model, but no tools, filesystem access, or execution privileges are granted.</p>
        <label class="searchbox"><Search size={18}/><input aria-label="Search agent roles" bind:value={search} oninput={() => (rolePageSize = 24)} placeholder="Search roles..."/></label>
        <div class="role-category-filters" role="group" aria-label="Filter agent roles by specialty">
          {#each ['All', ...roleGroups] as group}
            <button type="button" aria-pressed={roleCategory === group} class:chosen={roleCategory === group} onclick={() => { roleCategory = group; rolePageSize = 24; }}>{group}</button>
          {/each}
        </div>
        <p class="role-result-count" role="status">Showing {Math.min(rolePageSize, filteredRoles.length)} of {filteredRoles.length} matching roles · {starterRoles.length} total</p>
        <div class="roles-grid">
          {#each visibleRoles as role (role.name)}
            <div class="role-card"><div class="role-icon">{role.icon}</div><div class="role-group">{role.group}</div><h2>{role.name}</h2><p>{role.blurb}</p><button aria-label={`Create ${role.name} agent`} disabled={!isNative || working} title={!isNative ? 'Available in the installed desktop app' : 'Add to your team'} onclick={() => void createAgent(role.name, role.name)}>Add to team <Plus size={16}/></button></div>
          {:else}<div class="role-empty-state"><h2>No matching roles</h2><p>Try another specialty or search term.</p><button type="button" class="soft-button small" onclick={() => { search = ''; roleCategory = 'All'; rolePageSize = 24; }}>Clear role filters</button></div>{/each}
        </div>
        {#if visibleRoles.length < filteredRoles.length}
          <button type="button" class="soft-button role-show-more" onclick={() => (rolePageSize += 24)}>Show 24 more roles · {filteredRoles.length - visibleRoles.length} remaining</button>
        {/if}
        <p class="library-disclaimer">{starterRoles.length} distinct prompt-only roles. Fully operational tool-enabled agents are still being developed and no tools are granted by selecting a role.</p>
      </div>
    {:else}
      <header class="topbar"><div class="crumb"><strong>Settings</strong></div></header>
      <div class="content-page settings-page"><div class="eyebrow">MAKE IT YOURS</div><h1>Your space.<br /><em>Your rules.</em></h1><p class="page-lede">Connect a supported API format or local server. Provider availability and model capabilities depend on your endpoint.</p><div class="settings-block"><div><Cpu size={23}/><div><h2>AI connections</h2><p>Save models and select them for explicit requests.</p></div></div><button class="primary-button" onclick={openProviderDialog}><Plus size={16}/> Add provider</button></div>
        <div class="settings-block appearance-block">
          <div><SlidersHorizontal size={23}/><div><h2>Appearance</h2><p>Choose Constellation's Tidal Atelier light or dark palette. Your selection stays on this device.</p></div></div>
          <div class="theme-picker" role="group" aria-label="Appearance theme">
            <button type="button" aria-pressed={themeMode === 'system'} class:chosen={themeMode === 'system'} onclick={() => setTheme('system')}>System</button>
            <button type="button" aria-pressed={themeMode === 'light'} class:chosen={themeMode === 'light'} onclick={() => setTheme('light')}>Light</button>
            <button type="button" aria-pressed={themeMode === 'dark'} class:chosen={themeMode === 'dark'} onclick={() => setTheme('dark')}>Dark</button>
          </div>
        </div>
        <p class="provider-probe-note">Testing is optional and sends one small text prompt to the explicitly selected provider. An API may charge for this request. A passing test does not prove tool calling, streaming, or structured output.</p>
        {#if snapshot.providers.length}
          <div class="provider-directory-controls" role="search" aria-label="Find configured AI providers">
            <label for="provider-directory-search">Find a provider</label>
            <input id="provider-directory-search" type="search" placeholder="Name, model, endpoint…" autocomplete="off" bind:value={providerSearch} />
            <label for="provider-directory-status">Text-test status</label>
            <select id="provider-directory-status" bind:value={providerStatusFilter}>
              <option value="all">All</option>
              <option value="untested">Not tested in this session</option>
              <option value="text-compatible">Text test passed</option>
              <option value="attention">Text test needs attention</option>
            </select>
            <span role="status">{filteredProviders.length} of {snapshot.providers.length} profiles shown</span>
          </div>
        {/if}
        <div class="provider-list">
          {#each filteredProviders as provider (provider.id)}
            {@const result = probeResults[provider.id]}
            <div class="provider-row">
              <div class="provider-symbol"><Cpu size={18}/></div>
              <div><strong>{provider.name}</strong><span>{providerKindLabel(provider.kind)} · {provider.model}</span><span class="provider-endpoint" title={provider.endpoint}>{provider.endpoint}</span><span>{provider.remembered_key ? 'API key reference saved in OS credential manager' : 'No persistent API key in this profile'}</span><span class="provider-verification">{result?.status === 'compatible_text' ? 'Text test passed in this session only' : result ? 'Text test needs attention' : 'Not tested in this session'}</span></div>
              <div class="provider-actions">
                <button type="button" class="soft-button small" disabled={!isNative || working || !!probeBusyId} aria-label={'Revise configuration for ' + provider.name} onclick={() => openProviderRevision(provider)}>Revise</button>
                <button type="button" class="soft-button small" disabled={!isNative || working || !!probeBusyId} onclick={() => void testProvider(provider.id)}>{probeBusyId === provider.id ? 'Testing…' : 'Test text chat'}</button>
                {#if !provider.remembered_key}
                  <button type="button" class="text-button" disabled={!isNative || working || replyBusy || workflowBusy || !!probeBusyId} aria-label={'Remove unused connection ' + provider.name} onclick={() => (providerRemovalCandidate = providerRemovalCandidate === provider.id ? '' : provider.id)}>Remove unused</button>
                {:else}
                  <button type="button" class="text-button" disabled={!isNative || working || replyBusy || workflowBusy || !!probeBusyId} aria-label={'Forget stored API key for ' + provider.name} onclick={() => { providerForgetCandidate = providerForgetCandidate === provider.id ? '' : provider.id; providerForgetConfirmation = ''; }}>Forget saved key</button>
                {/if}
              </div>
            </div>
            {#if providerRemovalCandidate === provider.id}
              <div class="provider-remove-confirm" role="group" aria-label={'Confirm removal of ' + provider.name}>
                <p>Remove <strong>{provider.name}</strong> from this device? Only credential-free connections with no agent assignments, messages, tasks or saved workflows can be removed. The app will reject referenced or changed profiles. This cannot undo historical exports.</p>
                <button type="button" class="soft-button small" onclick={() => (providerRemovalCandidate = '')}>Keep connection</button>
                <button type="button" class="soft-button small" disabled={!isNative || working || replyBusy || workflowBusy || !!probeBusyId} onclick={() => void removeUnusedProvider(provider)}>Confirm remove unused</button>
              </div>
            {/if}
            {#if providerForgetCandidate === provider.id}
              <div class="provider-remove-confirm" role="group" aria-label={'Confirm forgetting stored API key for ' + provider.name}>
                <p>Delete the API key for <strong>{provider.name}</strong> from this operating system account? The provider profile, assigned agents, tasks, messages and workflow history will stay. New requests will need another key. Already-running requests may still complete; wait until they finish.</p>
                <label>Type the exact connection name to confirm
                  <input aria-label={'Confirm provider name ' + provider.name} type="text" autocomplete="off" spellcheck="false" bind:value={providerForgetConfirmation} />
                </label>
                <button type="button" class="soft-button small" onclick={() => { providerForgetCandidate = ''; providerForgetConfirmation = ''; }}>Keep saved key</button>
                <button type="button" class="soft-button small" disabled={!isNative || working || replyBusy || workflowBusy || !!probeBusyId || providerForgetConfirmation !== provider.name} onclick={() => void forgetRememberedProviderKey(provider)}>Delete saved API key</button>
              </div>
            {/if}
            {#if result}
              <p class="provider-probe-result" role="status">Text-only test: {result.status === 'compatible_text' ? 'Compatible for this one response' : result.status === 'request_error' ? result.message : result.status.replaceAll('_', ' ')}{#if 'http_status' in result && result.http_status} · HTTP {result.http_status}{/if}. No tool or streaming support was verified.</p>
            {/if}
          {:else}
            <div class="provider-none">{snapshot.providers.length ? 'No providers match these local filters. Clear the search or choose All.' : 'No AI providers are configured yet.'}</div>
          {/each}
        </div>
        <section class="agent-routing" aria-label="Assign models to agents">
          <h2>Models for individual agents</h2>
          <p>Set each agent’s preferred model. This saves a local preference only; nothing is sent until you explicitly choose individual model routing and press Ask or Run.</p>
          {#each snapshot.agents as agent (agent.id)}
            <label class="agent-route-row"><span><strong>{agent.name}</strong><small>{agent.role}</small></span>
              <select aria-label={'Preferred model for ' + agent.name} value={agent.provider_id ?? ''} disabled={!isNative || working || replyBusy} onchange={(event) => void assignAgentProvider(agent.id, event.currentTarget.value)}>
                <option value="">No model assigned</option>
                {#each snapshot.providers as provider (provider.id)}<option value={provider.id}>{provider.name} · {provider.model} ({providerKindLabel(provider.kind)})</option>{/each}
              </select>
            </label>
          {:else}
            <p>Add an agent in the Role library to configure individual model routing.</p>
          {/each}
        </section>
        <button class="advanced-toggle" aria-expanded={advanced} aria-controls={advanced ? 'advanced-studio-panel' : undefined} onclick={toggleAdvanced}><SlidersHorizontal size={17}/> Advanced Studio <ChevronDown size={16}/></button>
        {#if advanced}
          <div class="advanced-panel" id="advanced-studio-panel">
            <h2>Full project archive · Studio and workflows</h2>
            <p>Encrypt both local databases into one portable <code>.cstlproj</code> file, including your rooms, tasks, and saved workflow plans and results. Each database is snapshotted consistently; changes spanning both databases during creation might come from slightly different moments. Provider keys and attachments are excluded.</p>
            <form class="backup-form" aria-label="Create a full encrypted project archive" aria-busy={backupOperation === 'project-create'} onsubmit={(event) => { event.preventDefault(); void createProjectArchive(); }}>
              <label for="project-passphrase">Project archive passphrase</label><input id="project-passphrase" type="password" autocomplete="new-password" bind:value={projectPassphrase} disabled={backupBusy} required />
              <label for="project-confirm">Confirm project archive passphrase</label><input id="project-confirm" type="password" autocomplete="new-password" bind:value={projectConfirmation} disabled={backupBusy} required />
              <button type="submit" class="primary-button" disabled={!isNative || backupBusy || working || utf8Bytes(projectPassphrase) < 8 || utf8Bytes(projectPassphrase) > maxBackupPassphraseBytes || projectPassphrase !== projectConfirmation}>{backupOperation === 'project-create' ? 'Encrypting both databases…' : 'Create full project archive'}</button>
              {#if projectBackupError}<p role="alert" class="evidence-error">{projectBackupError}</p>{/if}
              {#if projectArchivePath}<div class="backup-path" role="status"><div><strong>Both databases archived</strong><code>{projectArchivePath}</code><span>Keep this file and its passphrase safe. Copy the archive to another device for disaster recovery.</span></div><button type="button" class="soft-button small" onclick={() => void copyLocalPath(projectArchivePath)}>Copy archive path</button></div>{/if}
            </form>
            <h2>Restore a full project archive</h2>
            <p>Choose the complete local path to a <code>.cstlproj</code> archive. Both databases are authenticated, checked and restored into a newly created directory. Your active project is never overwritten.</p>
            <form class="backup-form" aria-label="Restore a full encrypted project archive" aria-busy={backupOperation === 'project-restore'} onsubmit={(event) => { event.preventDefault(); void restoreProjectArchive(); }}>
              <label for="project-restore-path">Full project archive path</label><input id="project-restore-path" type="text" autocomplete="off" spellcheck="false" bind:value={projectRestorePath} disabled={backupBusy} required />
              <label for="project-restore-passphrase">Original project archive passphrase</label><input id="project-restore-passphrase" type="password" autocomplete="off" bind:value={projectRestorePassphrase} disabled={backupBusy} required />
              <button type="submit" class="soft-button" disabled={!isNative || backupBusy || working || !projectRestorePath.trim() || utf8Bytes(projectRestorePath.trim()) > maxRecoveryPathBytes || utf8Bytes(projectRestorePassphrase) < 8 || utf8Bytes(projectRestorePassphrase) > maxBackupPassphraseBytes}>{backupOperation === 'project-restore' ? 'Authenticating both databases…' : 'Recover full project to new directory'}</button>
              {#if projectRestoreError}<p role="alert" class="evidence-error">{projectRestoreError}</p>{/if}
              {#if projectRestoredDirectory}<div class="backup-path" role="status"><div><strong>Full project recovered</strong><code>{projectRestoredDirectory}</code><span>Contains studio.sqlite3 and workflows.sqlite3. Inspect these separately before any future project switch.</span></div><button type="button" class="soft-button small" onclick={() => void copyLocalPath(projectRestoredDirectory)}>Copy recovery directory</button></div>{/if}
            </form>
            <h2>Legacy studio-only backup and recovery</h2>
            <p>Make a passphrase-encrypted snapshot of committed rooms, messages, agents, and tasks. A new <code>.cstlbkp</code> file is saved in the app's recovery directory. Make a separate copy of the file to protect against losing this device. File attachments and provider API keys are excluded.</p>
            <form class="backup-form" aria-label="Create an encrypted workspace backup" aria-busy={backupOperation === 'create'} onsubmit={(event) => { event.preventDefault(); void createBackup(); }}>
              <p class="backup-field-note" id="backup-passphrase-help">Use a strong passphrase of 8–1024 UTF-8 bytes. You will need the exact same passphrase to recover the file; the app cannot reset it.</p>
              <label for="backup-password">New backup passphrase</label><input id="backup-password" type="password" autocomplete="new-password" aria-describedby="backup-passphrase-help" bind:value={backupPassphrase} disabled={backupBusy} required/>
              <label for="backup-confirm">Confirm backup passphrase</label><input id="backup-confirm" type="password" autocomplete="new-password" aria-describedby="backup-passphrase-help" bind:value={backupConfirmation} disabled={backupBusy} required/>
              {#if backupConfirmation && backupPassphrase !== backupConfirmation}<p class="evidence-error" role="status">Passphrases do not match.</p>{/if}
              <button type="submit" class="primary-button" disabled={!isNative || backupBusy || working || utf8Bytes(backupPassphrase) < 8 || utf8Bytes(backupPassphrase) > maxBackupPassphraseBytes || backupPassphrase !== backupConfirmation}>{backupOperation === 'create' ? 'Encrypting backup…' : 'Create encrypted backup'}</button>
              {#if backupOperation === 'create'}<p class="backup-field-note" role="status">Creating a new encrypted local file…</p>{/if}
              {#if backupCreateError}<p role="alert" class="evidence-error">{backupCreateError}</p>{/if}
              {#if backupResultPath}<div class="backup-path" role="status"><div><strong>Encrypted backup created</strong><code>{backupResultPath}</code><span>Save a copy of this package somewhere safe. The file contains workspace content protected by your passphrase.</span></div><button type="button" class="soft-button small" onclick={() => void copyLocalPath(backupResultPath)}>Copy path</button></div>{/if}
            </form>
            <h2>Recover to a separate file</h2>
            <p>Enter the full path of an existing <code>.cstlbkp</code> package and its passphrase. The app authenticates the package and writes a new SQLite database in the recovery directory. It does not open, merge, or overwrite your active workspace. Use the recovered file for inspection or a separate migration process.</p>
            <form class="backup-form" aria-label="Recover an encrypted backup" aria-busy={backupOperation === 'restore'} onsubmit={(event) => { event.preventDefault(); void restoreBackup(); }}>
              <label for="restore-path">Full path to the encrypted backup</label><input id="restore-path" type="text" spellcheck="false" autocomplete="off" aria-describedby="restore-path-help" bind:value={restorePackagePath} disabled={backupBusy} required/>
              <p class="backup-field-note" id="restore-path-help">Paste the complete local path to a <code>.cstlbkp</code> file, up to 4096 UTF-8 bytes. No files are uploaded.</p>
              <label for="restore-password">Original backup passphrase</label><input id="restore-password" type="password" autocomplete="off" bind:value={restorePassphrase} disabled={backupBusy} required/>
              <button type="submit" class="soft-button" disabled={!isNative || backupBusy || working || !restorePackagePath.trim() || utf8Bytes(restorePackagePath.trim()) > maxRecoveryPathBytes || utf8Bytes(restorePassphrase) < 8 || utf8Bytes(restorePassphrase) > maxBackupPassphraseBytes}>{backupOperation === 'restore' ? 'Verifying and recovering…' : 'Verify and recover to a new file'}</button>
              {#if backupOperation === 'restore'}<p class="backup-field-note" role="status">Verifying the encrypted package and creating a separate database…</p>{/if}
              {#if backupRestoreError}<p role="alert" class="evidence-error">{backupRestoreError}</p>{/if}
              {#if restoredDatabasePath}<div class="backup-path" role="status"><div><strong>Recovery file created</strong><code>{restoredDatabasePath}</code><span>Your active workspace is unchanged. The recovered SQLite file requires separate inspection or migration before its data can be used here.</span></div><button type="button" class="soft-button small" onclick={() => void copyLocalPath(restoredDatabasePath)}>Copy path</button></div>{/if}
            </form>
            <p class="advanced-limitations">Plan, validate, save and cancel multi-agent workflows from the Tasks → Workflows view. Workflow execution, granular tool permissions, runner pairing and encrypted synchronization are still in development.</p>
          </div>
        {/if}
      </div>
    {/if}
  </main>

  {#if showDetails && screen === 'rooms' && !focusMode}<aside class="details" aria-label="Room information"><div class="details-heading"><strong>Room details</strong><button class="icon-button" aria-label="Close details" onclick={() => (showDetails = false)}><X size={18}/></button></div><div class="details-section"><span class="eyebrow">CURRENT SPACE</span><h2>{room?.name || 'No room selected'}</h2><p>{room?.description || 'Bring people and agents together to work.'}</p>{#if room}<button class="soft-button small" disabled={!isNative} onclick={() => void copyTranscript()}>Copy transcript</button>{/if}</div><div class="details-section"><span class="eyebrow">EXECUTION</span><div class="details-row"><Cpu size={18}/> This device</div><p>There is no remote runner configured.</p></div><div class="details-section"><span class="eyebrow">CONNECTED AGENTS</span><p>{snapshot.agents.length} available teammates</p><button class="soft-button small" onclick={() => (dialog = 'new-agent')}><Plus size={14}/> Add agent</button></div></aside>{/if}
</div>

{#if dialog}
  <div class="modal-backdrop">
    <div class="modal" class:command-dialog={dialog === 'commands'} role="dialog" aria-modal="true" aria-label={dialog === 'new-room' ? 'Create room' : dialog === 'new-agent' ? 'Create agent' : dialog === 'new-task' ? 'Plan task' : dialog === 'provider' ? 'Connect AI provider' : 'Search workspace'} use:focusDialog>
      <div class="modal-heading"><div class="modal-icon">{#if dialog === 'new-room'}<MessageCircle size={21}/>{:else if dialog === 'new-agent'}<Bot size={21}/>{:else if dialog === 'new-task'}<Layers3 size={21}/>{:else if dialog === 'provider'}<Cpu size={21}/>{:else}<Command size={21}/>{/if}</div><button class="icon-button" aria-label="Close dialog" onclick={closeDialog}><X size={20}/></button></div>
      {#if dialog === 'new-room'}<h2>A space for what’s next.</h2><p>Start with a room name. Invite agents when you're ready.</p><form onsubmit={(event) => {event.preventDefault();void createRoom();}}><label>ROOM NAME<input bind:value={newRoom} placeholder="e.g. Research lab, My next product" maxlength="80" required/></label><button class="primary-button full" disabled={working || !newRoom.trim() || !isNative} type="submit">Create room <ArrowRight size={17}/></button></form>
      {:else if dialog === 'new-agent'}<h2>Meet your next teammate.</h2><p>Choose a role and a name. Permissions and model capabilities still determine what an agent can actually do.</p><form onsubmit={(event) => {event.preventDefault();void createAgent(selectedRole,newAgent);}}><label>ROLE<select bind:value={selectedRole}>{#each starterRoles as role}<option value={role.name}>{role.name}</option>{/each}</select></label><label>DISPLAY NAME<input bind:value={newAgent} placeholder="e.g. Avery" maxlength="80"/></label><button class="primary-button full" disabled={working || !isNative} type="submit">Add teammate <ArrowRight size={17}/></button></form>
      {:else if dialog === 'new-task'}<h2>Plan your next task.</h2><p>Save instructions for a teammate. Text-only model execution is available with an explicitly selected provider; local file actions still require separate native approval.</p>
        {#if !snapshot.rooms.length || !snapshot.agents.length}
          <div class="task-setup-guide" role="group" aria-label="Complete task prerequisites">
            <strong>Complete your setup to save this plan</strong>
            {#if !snapshot.rooms.length}<button type="button" class="soft-button small" onclick={() => continueTaskSetup('new-room')}><Plus size={14}/> Create a room first</button>{/if}
            {#if !snapshot.agents.length}<button type="button" class="soft-button small" onclick={() => continueTaskSetup('new-agent')}><Plus size={14}/> Add a teammate</button>{/if}
            <span>Your instructions stay in the form while you complete setup.</span>
          </div>
        {/if}
        <form onsubmit={(event) => {event.preventDefault();void createTask();}}><label>ROOM<select bind:value={taskRoomId} required>{#each snapshot.rooms as r (r.id)}<option value={r.id}>{r.name}</option>{/each}</select></label><label>TEAMMATE<select bind:value={taskAgentId} required>{#each snapshot.agents as agent (agent.id)}<option value={agent.id}>{agent.name} — {agent.role}</option>{/each}</select></label><label>INSTRUCTIONS<textarea bind:value={taskInstructions} rows="4" maxlength="10000" required placeholder="Describe the outcome you want..."></textarea></label><button class="primary-button full" type="submit" disabled={!isNative || working || !taskRoomId || !taskAgentId || !taskInstructions.trim()}>Save pending task <ArrowRight size={17}/></button></form>
      {:else if dialog === 'provider'}
        <h2>{providerRevisionSourceId ? 'Revise a connection.' : 'Bring your own model.'}</h2>
        <p>{providerRevisionSourceId ? 'Review this saved configuration. Keys are never shown or copied. Unchanged profile details reuse the original connection; changing its name, model, endpoint or API format creates a separate profile and leaves the original intact.' : 'Choose the API format this provider actually supports. Saving a connection does not contact the provider or send messages.'}</p>
        {#if providerRevisionSourceId}
          <p class="provider-revision-note" role="note">{providerRevisionSavedKey ? 'This profile has a securely remembered credential. Re-enter a replacement key before saving; leave this dialog to retain the existing credential unchanged.' : 'Enter the API key again if the connection needs one. Saving without a key clears an existing session-only key when all other profile details match.'} No provider request is sent by saving.</p>
        {/if}
        <form onsubmit={(event) => {event.preventDefault();void saveProvider();}}>
          <label for="provider-kind">API FORMAT
            <select id="provider-kind" value={providerKind} disabled={working} onchange={(event) => chooseProviderKind(event.currentTarget.value as ProviderKind)}>
              {#each Object.keys(providerPresets) as kind (kind)}<option value={kind}>{providerKindLabel(kind)}</option>{/each}
            </select>
          </label>
          <p id="provider-kind-help" class="provider-kind-help" aria-live="polite">{providerPreset.detail}</p>
          <label for="provider-name">DISPLAY NAME<input id="provider-name" bind:value={providerName} maxlength="120" autocomplete="off" required /></label>
          <label for="provider-endpoint">API BASE URL<input id="provider-endpoint" type="url" value={providerEndpoint} oninput={(event) => { providerEndpoint = event.currentTarget.value; providerKey = ''; providerRememberKey = false; }} placeholder={providerPreset.endpoint} spellcheck="false" autocapitalize="off" autocomplete="off" required /></label>
          <p class="provider-field-hint">The URL is a base URL, not a full request path. Remote connections require HTTPS; HTTP is permitted only for localhost.</p>
          <label for="provider-model">MODEL IDENTIFIER<input id="provider-model" bind:value={providerModel} placeholder={providerPreset.modelHint} spellcheck="false" autocapitalize="off" autocomplete="off" required /></label>
          <label for="provider-key">API KEY {providerKind === 'ollama_native' ? '(USUALLY NOT NEEDED LOCALLY)' : '(IF REQUIRED BY PROVIDER)'}
            <input id="provider-key" type="password" autocomplete="off" bind:value={providerKey} placeholder={providerPreset.keyHint} spellcheck="false" />
          </label>
          <label class="provider-remember"><input type="checkbox" bind:checked={providerRememberKey} disabled={!providerKey.trim() || working}/> Remember this API key in the operating system's secure credential manager</label>
          <p class="provider-field-hint">By default, keys are held only in this app session. Opt-in secure storage is available on supported Windows/macOS accounts and may be accessible to other programs running under the same account. Other platforms reject the request without a plaintext fallback. Do not put keys in the URL; switching API formats clears the entered key.</p>
          <button class="primary-button full" disabled={working || !providerName.trim() || !providerEndpoint.trim() || !providerModel.trim() || !isNative || (providerRevisionSourceId !== '' && providerRevisionSavedKey && !providerKey.trim())} type="submit">{providerRevisionSourceId ? 'Save revised configuration' : 'Save connection locally'} <ArrowRight size={17}/></button>
        </form>
        <p class="dialog-footnote"><ShieldCheck size={14}/> Provider compatibility is not guaranteed. Only explicit Ask, Run, or Test actions can send model requests; remote services may charge.</p>
      {:else}
        <WorkspaceCommandPalette rooms={snapshot.rooms} onNavigate={navigate} onSelectRoom={openCommandRoom} onAction={openCommandAction}/>
      {/if}
      {#if !isNative}<p class="dialog-footnote" role="status">This is a browser preview. Changes require the installed desktop app.</p>{/if}
    </div>
  </div>
{/if}
