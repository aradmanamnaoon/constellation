/**
 * Serializes dependent local UI work. Every caller waits for its own task;
 * a rejected task does not poison the queue or suppress later refreshes.
 *
 * Queue order reflects invocation order. This avoids an older, slower
 * bootstrap response replacing a newer snapshot or overwriting scroll state.
 */
export class SerialAsyncQueue {
  private tail: Promise<void> = Promise.resolve();

  enqueue<T>(work: () => Promise<T>): Promise<T> {
    const result = this.tail.then(work);
    this.tail = result.then(() => undefined, () => undefined);
    return result;
  }
}
