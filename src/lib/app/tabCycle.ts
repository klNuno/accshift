/** Next tab for mod+tab / mod+shift+tab. When the active tab is not in the
 *  usable list (disabled or unsupported), forward starts at the first tab and
 *  backward at the last one. Null when there is nothing to cycle to. */
export function pickCycledTab(
  usableIds: readonly string[],
  activeId: string,
  direction: 1 | -1,
): string | null {
  const count = usableIds.length;
  if (count < 2) return null;
  const index = usableIds.indexOf(activeId);
  if (index === -1) return direction === 1 ? usableIds[0] : usableIds[count - 1];
  return usableIds[(index + direction + count) % count];
}
