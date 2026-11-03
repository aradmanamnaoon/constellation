export type WorkspaceSection = 'home' | 'rooms' | 'tasks' | 'evidence' | 'library' | 'settings';
export type WorkspaceRoom = { id: string; name: string };
export type WorkspaceAction = 'new-room' | 'new-agent' | 'provider';
export type WorkspaceCommand =
  | { id: string; type: 'section'; label: string; description: string; section: WorkspaceSection; keywords: string }
  | { id: string; type: 'room'; label: string; description: string; roomId: string; keywords: string }
  | { id: string; type: 'action'; label: string; description: string; action: WorkspaceAction; keywords: string };

const sections: WorkspaceCommand[] = [
  { id: 'section:home', type: 'section', label: 'Home', description: 'Overview and getting started', section: 'home', keywords: 'dashboard start' },
  { id: 'section:rooms', type: 'section', label: 'Rooms', description: 'Conversations with your AI team', section: 'rooms', keywords: 'chat messages conversation' },
  { id: 'section:tasks', type: 'section', label: 'Tasks and workflows', description: 'Plans, progress and execution', section: 'tasks', keywords: 'tasks workflows runs' },
  { id: 'section:evidence', type: 'section', label: 'Evidence', description: 'Saved documents and local search', section: 'evidence', keywords: 'files sources citations' },
  { id: 'section:library', type: 'section', label: 'Role library', description: 'Discover specialist roles', section: 'library', keywords: 'agents people roles' },
  { id: 'section:settings', type: 'section', label: 'Settings', description: 'Providers, privacy and recovery', section: 'settings', keywords: 'models keys backups' },
];

const actions: WorkspaceCommand[] = [
  { id: 'action:new-room', type: 'action', label: 'Create a room', description: 'Start a new conversation', action: 'new-room', keywords: 'new chat conversation' },
  { id: 'action:new-agent', type: 'action', label: 'Add an agent', description: 'Choose a role for your team', action: 'new-agent', keywords: 'new teammate person role' },
  { id: 'action:provider', type: 'action', label: 'Connect a model', description: 'Configure a local or remote provider', action: 'provider', keywords: 'new provider api settings' },
];

function normalize(value: string): string {
  return value.trim().toLocaleLowerCase();
}

export function filterWorkspaceRooms<T extends WorkspaceRoom>(rooms: readonly T[], query: string): T[] {
  const terms = normalize(query).split(/\s+/u).filter(Boolean);
  if (!terms.length) return [...rooms];
  return rooms.filter((room) => terms.every((term) => normalize(room.name).includes(term)));
}

export function searchWorkspaceCommands(rooms: readonly WorkspaceRoom[], query: string, limit = 30): WorkspaceCommand[] {
  const roomCommands: WorkspaceCommand[] = rooms.map((room) => ({
    id: `room:${room.id}`, type: 'room', label: room.name,
    description: 'Open conversation', roomId: room.id, keywords: 'room chat conversation',
  }));
  const all = [...sections, ...actions, ...roomCommands];
  const terms = normalize(query).split(/\s+/u).filter(Boolean);
  if (!terms.length) return [...sections, ...actions, ...roomCommands.slice(0, 8)].slice(0, limit);
  return all
    .filter((command) => {
      const text = normalize(`${command.label} ${command.description} ${command.keywords}`);
      return terms.every((term) => text.includes(term));
    })
    .sort((left, right) => {
      const phrase = terms.join(' ');
      const score = (command: WorkspaceCommand): number => {
        const name = normalize(command.label);
        return name === phrase ? 0 : name.startsWith(phrase) ? 1 : name.includes(phrase) ? 2 : 3;
      };
      return score(left) - score(right);
    })
    .slice(0, Math.max(0, limit));
}
