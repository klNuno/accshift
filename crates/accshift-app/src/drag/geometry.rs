//! Where a dropped card lands: the slot rects measured at drag start and the
//! index the pointer points at among them.

/// The part of a slot's on-screen rect the drop calculation reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// The index the dragged card would take if dropped at `(x, y)`, `None`
/// when the layout offers nowhere to drop (no slots, or the card's own
/// index was never resolved).
///
/// The pointer picks the nearest slot centre, then the side it fell on
/// decides before or after that slot. Dropping past the old position
/// shifts the target back by one: removing the card first closes its gap.
pub fn preview_index_at(
    x: f32,
    y: f32,
    slots: &[SlotRect],
    old_index: Option<usize>,
    list_mode: bool,
) -> Option<usize> {
    let old_index = old_index?;
    if slots.is_empty() {
        return None;
    }
    let mut nearest = 0;
    let mut nearest_distance = f32::INFINITY;
    for (i, slot) in slots.iter().enumerate() {
        let dx = x - (slot.left + slot.width / 2.0);
        let dy = y - (slot.top + slot.height / 2.0);
        let distance = dx * dx + dy * dy;
        if distance < nearest_distance {
            nearest_distance = distance;
            nearest = i;
        }
    }
    let slot = slots[nearest];
    // A list stacks vertically, a grid flows horizontally: the axis the
    // cards advance along decides before or after.
    let past = if list_mode {
        y > slot.top + slot.height / 2.0
    } else {
        x > slot.left + slot.width / 2.0
    };
    let drop_index = if past { nearest + 1 } else { nearest };
    let insert_at = if drop_index > old_index {
        drop_index - 1
    } else {
        drop_index
    };
    Some(insert_at.min(slots.len() - 1))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Four 100 px slots side by side.
    pub(in crate::drag) fn row() -> Vec<SlotRect> {
        (0..4)
            .map(|i| SlotRect {
                left: i as f32 * 100.0,
                top: 0.0,
                width: 100.0,
                height: 100.0,
            })
            .collect()
    }

    fn column() -> Vec<SlotRect> {
        (0..4)
            .map(|i| SlotRect {
                left: 0.0,
                top: i as f32 * 100.0,
                width: 100.0,
                height: 100.0,
            })
            .collect()
    }

    #[test]
    fn has_nowhere_to_drop_when_the_layout_was_never_measured() {
        assert_eq!(preview_index_at(50.0, 50.0, &[], Some(0), false), None);
        assert_eq!(preview_index_at(50.0, 50.0, &row(), None, false), None);
    }

    #[test]
    fn drops_before_or_after_the_nearest_card() {
        assert_eq!(
            preview_index_at(210.0, 50.0, &row(), Some(0), false),
            Some(1)
        );
        assert_eq!(
            preview_index_at(260.0, 50.0, &row(), Some(0), false),
            Some(2)
        );
    }

    #[test]
    fn reads_the_vertical_axis_in_list_mode() {
        assert_eq!(
            preview_index_at(60.0, 240.0, &column(), Some(3), true),
            Some(2)
        );
        assert_eq!(
            preview_index_at(60.0, 260.0, &column(), Some(3), true),
            Some(3)
        );
    }

    #[test]
    fn closes_the_gap_the_card_left_behind() {
        assert_eq!(
            preview_index_at(260.0, 50.0, &row(), Some(0), false),
            Some(2)
        );
        assert_eq!(
            preview_index_at(30.0, 50.0, &row(), Some(3), false),
            Some(0)
        );
    }

    #[test]
    fn never_points_outside_the_list() {
        assert_eq!(
            preview_index_at(-5000.0, 50.0, &row(), Some(2), false),
            Some(0)
        );
        assert_eq!(
            preview_index_at(5000.0, 50.0, &row(), Some(0), false),
            Some(3)
        );
    }

    #[test]
    fn resolves_a_pointer_inside_no_card() {
        let gapped: Vec<SlotRect> = (0..4)
            .map(|i| SlotRect {
                left: i as f32 * 120.0,
                top: 0.0,
                width: 100.0,
                height: 100.0,
            })
            .collect();
        assert_eq!(
            preview_index_at(115.0, 50.0, &gapped, Some(3), false),
            Some(1)
        );
    }
}
