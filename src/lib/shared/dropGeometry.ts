/**
 * Where a dragged card would land, from geometry alone.
 *
 * Split out of the drag manager because it is the one part of a drag that
 * needs no DOM, no reactive state and no folder store: given the slot
 * rectangles captured when the drag started and a pointer position, the answer
 * is arithmetic. That makes it the part a test can actually pin down.
 */

/** The part of a `DOMRect` the drop calculation reads. */
export interface SlotRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Index the dragged card would take if it were dropped here, or `null` when
 * the layout offers nowhere to drop: an empty grid, or a card whose own index
 * was never resolved.
 *
 * The pointer picks the nearest slot centre, then the side it fell on decides
 * whether the card goes before or after that slot. Dropping past your own old
 * position shifts the target back by one, because removing the card first
 * closes the gap it left behind.
 */
export function previewIndexAt(
  clientX: number,
  clientY: number,
  slots: readonly SlotRect[],
  oldIndex: number,
  listMode: boolean,
): number | null {
  if (slots.length === 0 || oldIndex < 0) return null;

  let nearest = 0;
  let nearestDistance = Infinity;
  for (let i = 0; i < slots.length; i++) {
    const slot = slots[i];
    const distance =
      (clientX - (slot.left + slot.width / 2)) ** 2 + (clientY - (slot.top + slot.height / 2)) ** 2;
    if (distance < nearestDistance) {
      nearestDistance = distance;
      nearest = i;
    }
  }

  const slot = slots[nearest];
  // A list stacks vertically, a grid flows horizontally, so the axis that
  // decides "before or after" is the one the cards advance along.
  const past = listMode
    ? clientY > slot.top + slot.height / 2
    : clientX > slot.left + slot.width / 2;
  const dropIndex = past ? nearest + 1 : nearest;
  const insertAt = dropIndex > oldIndex ? dropIndex - 1 : dropIndex;
  return Math.max(0, Math.min(insertAt, slots.length - 1));
}
