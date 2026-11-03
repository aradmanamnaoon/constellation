/** Local-only provider directory discovery. Never probes networks or reads keys. */
export type ProviderStatusFilter = 'all' | 'untested' | 'text-compatible' | 'attention';

export type DirectoryProvider = {
  id: string;
  name: string;
  model: string;
  endpoint: string;
  kind?: string;
};

export type ProbeSummary = { status: string };

export function filterProviderDirectory<T extends DirectoryProvider>(
  providers: readonly T[],
  probes: Readonly<Record<string, ProbeSummary | undefined>>,
  search: string,
  status: ProviderStatusFilter,
): T[] {
  const tokens = search.trim().toLocaleLowerCase().split(/\s+/u).filter(Boolean);
  return providers.filter((provider) => {
    const probe = probes[provider.id];
    if (status === 'untested' && probe) return false;
    if (status === 'text-compatible' && probe?.status !== 'compatible_text') return false;
    if (status === 'attention' && (!probe || probe.status === 'compatible_text')) return false;
    if (!tokens.length) return true;
    const content = `${provider.name} ${provider.model} ${provider.endpoint} ${provider.kind ?? ''}`.toLocaleLowerCase();
    return tokens.every((token) => content.includes(token));
  });
}
