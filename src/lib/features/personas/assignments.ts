import type { PersonaAssignment } from "./types";

/**
 * Assignments to write when a persona is saved.
 *
 * Platforms the wizard is not offering, and platforms whose account read
 * failed, keep the stored assignment. The user removes one of those only by
 * clearing it (`selection` is ""). A platform whose accounts loaded
 * successfully drops an id that is no longer in that list.
 *
 * Original assignments stay in their stored order so an unchanged edit compares
 * equal. Accounts picked on a platform that had none are appended in wizard order.
 */
export function personaAssignmentsForSave(input: {
  original: readonly PersonaAssignment[];
  offeredPlatformIds: readonly string[];
  selection: Readonly<Record<string, string>>;
  accountsByPlatform: Readonly<Record<string, readonly { id: string }[]>>;
  loading: boolean;
  failedPlatformIds: readonly string[];
}): PersonaAssignment[] {
  const offered = new Set(input.offeredPlatformIds);
  const failed = new Set(input.failedPlatformIds);
  const result: PersonaAssignment[] = [];
  const seen = new Set<string>();

  for (const assignment of input.original) {
    seen.add(assignment.platformId);
    const edited = Object.prototype.hasOwnProperty.call(input.selection, assignment.platformId);
    if (edited && input.selection[assignment.platformId] === "") continue;

    if (!offered.has(assignment.platformId)) {
      result.push(assignment);
      continue;
    }

    const selected = edited ? input.selection[assignment.platformId] : "";
    const id = selected || assignment.accountId;
    if (!id) continue;
    if (!input.loading && !failed.has(assignment.platformId)) {
      const accounts = input.accountsByPlatform[assignment.platformId] ?? [];
      if (!accounts.some((account) => account.id === id)) continue;
    }
    result.push(
      id === assignment.accountId
        ? assignment
        : { platformId: assignment.platformId, accountId: id },
    );
  }

  for (const platformId of input.offeredPlatformIds) {
    if (seen.has(platformId)) continue;
    const id = input.selection[platformId];
    if (!id) continue;
    if (!input.loading && !failed.has(platformId)) {
      const accounts = input.accountsByPlatform[platformId] ?? [];
      if (!accounts.some((account) => account.id === id)) continue;
    }
    result.push({ platformId, accountId: id });
  }

  return result;
}
