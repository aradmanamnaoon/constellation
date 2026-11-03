<script lang="ts">
  import { tick } from 'svelte';
  import { ArrowRight, Search } from '@lucide/svelte';
  import {
    searchWorkspaceCommands,
    type WorkspaceAction,
    type WorkspaceCommand,
    type WorkspaceRoom,
    type WorkspaceSection,
  } from './workspace-navigation';

  type Props = {
    rooms: WorkspaceRoom[];
    onNavigate: (section: WorkspaceSection) => void;
    onSelectRoom: (id: string) => void;
    onAction: (action: WorkspaceAction) => void;
  };

  let { rooms, onNavigate, onSelectRoom, onAction }: Props = $props();
  let query = $state('');
  let selectedIndex = $state(0);
  const commands = $derived(searchWorkspaceCommands(rooms, query));
  const currentIndex = $derived(Math.min(selectedIndex, Math.max(0, commands.length - 1)));

  function activate(command: WorkspaceCommand) {
    if (command.type === 'section') onNavigate(command.section);
    else if (command.type === 'room') onSelectRoom(command.roomId);
    else onAction(command.action);
  }

  async function revealActiveOption() {
    await tick();
    document.getElementById(`workspace-command-${currentIndex}`)?.scrollIntoView({ block: 'nearest' });
  }

  function onSearchKeydown(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!commands.length) return;
      selectedIndex = (currentIndex + (event.key === 'ArrowDown' ? 1 : -1) + commands.length) % commands.length;
      void revealActiveOption();
    } else if (event.key === 'Home' && !query) {
      event.preventDefault();
      selectedIndex = 0;
      void revealActiveOption();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const selected = commands[currentIndex];
      if (selected) activate(selected);
    }
  }
</script>

<div class="workspace-command-palette">
  <h2 id="workspace-command-title">Go to anything</h2>
  <p id="workspace-command-hint">Find a room, open a section, or start a setup action. Use ↑ ↓ to choose and Enter to open.</p>
  <div class="workspace-command-search">
    <Search size={19} aria-hidden="true"/>
    <input
      aria-label="Search workspace commands and rooms"
      role="combobox"
      aria-autocomplete="list"
      aria-expanded="true"
      aria-controls="workspace-command-results"
      aria-describedby="workspace-command-hint"
      aria-activedescendant={commands.length ? `workspace-command-${currentIndex}` : undefined}
      placeholder="Search rooms, tasks, settings…"
      autocomplete="off"
      bind:value={query}
      oninput={() => (selectedIndex = 0)}
      onkeydown={onSearchKeydown}
    />
    <kbd>Ctrl K</kbd>
  </div>
  <div id="workspace-command-results" role="listbox" aria-label="Workspace destinations" class="workspace-command-results">
    {#each commands as command, index (command.id)}
      <button
        id={`workspace-command-${index}`}
        type="button"
        role="option"
        tabindex="-1"
        aria-selected={index === currentIndex}
        class:highlighted={index === currentIndex}
        onclick={() => activate(command)}
        onpointermove={() => (selectedIndex = index)}
      >
        <span class="workspace-command-description"><strong>{command.label}</strong><small>{command.description}</small></span>
        <span class="workspace-command-category">{command.type === 'room' ? 'Room' : command.type === 'action' ? 'Action' : 'Page'}</span>
        <ArrowRight size={15} aria-hidden="true"/>
      </button>
    {:else}
      <p class="workspace-command-empty">No matching destinations. Try the room name or a section such as Evidence.</p>
    {/each}
  </div>
  <p class="workspace-command-footer" role="status">{commands.length} {commands.length === 1 ? 'result' : 'results'} · Esc to close · No AI requests are sent</p>
</div>
