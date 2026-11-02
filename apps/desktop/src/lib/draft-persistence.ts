/** Native-only per-room draft queue. No browser storage, model or network calls. */
export type StoredDraft = { room_id: string; content: string; revision: number };
export type DraftState = 'saved' | 'unsaved' | 'saving' | 'error';
export type DraftWriter = (roomId: string, content: string, expectedRevision: number) => Promise<StoredDraft>;

type Entry = {
  text: string;
  savedText: string;
  revision: number;
  pending: Promise<boolean> | null;
  timer: ReturnType<typeof setTimeout> | null;
};

export class DraftPersistence {
  private readonly entries = new Map<string, Entry>();
  private readonly save: DraftWriter;
  private readonly report: (roomId: string, state: DraftState, error?: string) => void;
  private readonly delayMs: number;

  constructor(
    save: DraftWriter,
    report: (roomId: string, state: DraftState, error?: string) => void,
    delayMs = 180,
  ) {
    this.save = save;
    this.report = report;
    this.delayMs = delayMs;
  }

  hydrate(rows: readonly StoredDraft[]): void {
    for (const row of rows) {
      if (!row.room_id || !Number.isSafeInteger(row.revision) || row.revision < 1) {
        throw new Error('Invalid native draft revision');
      }
      this.replaceFromDisk(row.room_id, row);
    }
  }

  replaceFromDisk(roomId: string, row?: StoredDraft): void {
    const existing = this.entries.get(roomId);
    if (existing?.pending) throw new Error('Cannot discard a draft during an active save');
    if (existing?.timer) clearTimeout(existing.timer);
    this.entries.set(roomId, {
      text: row?.content ?? '', savedText: row?.content ?? '', revision: row?.revision ?? 0,
      timer: null, pending: null,
    });
    this.report(roomId, 'saved');
  }

  text(roomId: string): string { return this.entries.get(roomId)?.text ?? ''; }

  change(roomId: string, value: string, immediately = false): void {
    if (!roomId) return;
    let entry = this.entries.get(roomId);
    if (!entry) {
      entry = { text: '', savedText: '', revision: 0, pending: null, timer: null };
      this.entries.set(roomId, entry);
    }
    entry.text = value;
    if (entry.timer) clearTimeout(entry.timer);
    entry.timer = null;
    if (entry.text === entry.savedText && !entry.pending) {
      this.report(roomId, 'saved');
      return;
    }
    this.report(roomId, 'unsaved');
    if (immediately) { void this.flush(roomId); }
    else entry.timer = setTimeout(() => { void this.flush(roomId); }, this.delayMs);
  }

  async flush(roomId: string): Promise<boolean> {
    const entry = this.entries.get(roomId);
    if (!entry) return true;
    if (entry.timer) clearTimeout(entry.timer);
    entry.timer = null;
    if (entry.pending) {
      const first = await entry.pending;
      return first && this.flush(roomId);
    }
    if (entry.text === entry.savedText) {
      this.report(roomId, 'saved');
      return true;
    }
    const content = entry.text;
    const revision = entry.revision;
    this.report(roomId, 'saving');
    const savePromise = (async (): Promise<boolean> => {
      try {
        const result = await this.save(roomId, content, revision);
        if (result.room_id !== roomId || result.content !== content || result.revision !== revision + 1) {
          throw new Error('Native draft confirmation was inconsistent');
        }
        entry.revision = result.revision;
        entry.savedText = content;
        return true;
      } catch (cause) {
        this.report(roomId, 'error', String(cause));
        return false;
      } finally {
        entry.pending = null;
      }
    })();
    entry.pending = savePromise;
    const saved = await savePromise;
    if (!saved) return false;
    if (entry.text !== entry.savedText) return this.flush(roomId);
    this.report(roomId, 'saved');
    return true;
  }

  hasUnsaved(): boolean {
    return [...this.entries.values()].some((entry) => entry.text !== entry.savedText || !!entry.pending);
  }

  dispose(): void {
    for (const entry of this.entries.values()) {
      if (entry.timer) clearTimeout(entry.timer);
      entry.timer = null;
    }
  }
}
