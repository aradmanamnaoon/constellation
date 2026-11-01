export type RoleSummary = { name: string; group: string; blurb: string };

export function roleCategories(roles: readonly RoleSummary[]): string[] {
  return [...new Set(roles.map(({ group }) => group.trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));
}

export function filterRoleLibrary<T extends RoleSummary>(
  roles: readonly T[],
  query: string,
  category: string,
): T[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/u).filter(Boolean);
  return roles.filter((role) =>
    (category === 'All' || role.group === category)
    && terms.every((term) => [role.name, role.group, role.blurb].some((field) => field.toLocaleLowerCase().includes(term))),
  );
}
