/** UI-only pagination. Never changes the underlying task order or executes work. */
export function paginateLocal<T>(items: readonly T[], requestedPage: number, pageSize = 40) {
  if (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > 100) {
    throw new RangeError('Invalid local page size');
  }
  const lastPage = Math.max(0, Math.ceil(items.length / pageSize) - 1);
  const page = Number.isSafeInteger(requestedPage)
    ? Math.min(Math.max(0, requestedPage), lastPage)
    : 0;
  const start = page * pageSize;
  return {
    items: items.slice(start, start + pageSize),
    page,
    lastPage,
    start,
    total: items.length,
  };
}
