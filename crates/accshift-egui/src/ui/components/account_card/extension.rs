//! The extension panel under the card: which side it opens to, whether it
//! shows, and its surface.

use std::ops::Range;

use egui::Ui;

use accshift_app::platforms::card::has_card_extension_content;

use super::input::Hover;
use super::{Card, State};
use crate::ui::components::card_extension::{self, ChipHit, Surface};

#[derive(Clone, Copy, Debug)]
pub(super) struct Extension {
    /// The platform gave the card something to show.
    pub has: bool,
    /// The panel shows this frame.
    pub visible: bool,
}

impl Extension {
    /// Picks the panel's side when a hover starts or the panel is forced
    /// open, and whether it shows: forced open, or hovered while hovering
    /// may open it; never while dragged or disabled.
    pub fn update(card: &Card, st: &mut State, hover: Hover) -> Self {
        let props = card.props;
        let has = has_card_extension_content(props.extension);
        let forced = props.extension_forced && has;
        if has && (hover.entered || (forced && !st.forced)) {
            // The room right of the card in window px: the stage's zoom
            // included, the panel's own width not.
            let ctx = &card.ctx;
            let zoom = crate::zoom::frame(ctx);
            st.opens_left = !card_extension::opens_right(
                ctx.content_rect().width() * zoom,
                card.rect().right() * zoom,
            );
        }
        st.forced = forced;
        let visible = has
            && !card.item().dragged
            && !props.extension_disabled
            && (props.extension_forced || (!props.extension_hover_disabled && hover.hovered));
        Self { has, visible }
    }

    /// Paints the surface under the face. Returns the range of its shapes
    /// in the ui's layer, when it showed, and its clickable chips.
    pub fn paint(
        self,
        ui: &mut Ui,
        card: &Card,
        st: &State,
        hovered: bool,
    ) -> (Option<Range<usize>>, Vec<ChipHit>) {
        if !self.has {
            return (None, Vec::new());
        }
        let props = card.props;
        let item = card.item();
        let layer = ui.layer_id();
        let next = || {
            card.ctx
                .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0))
        };
        let start = next();
        let painted = card_extension::paint(
            ui,
            card.layout(),
            &Surface {
                vars: props.vars,
                id: props.id,
                sections: props.extension,
                clickable: props.clickable_chips,
                locale: props.locale,
                card_color: props.card_color,
                active: item.active,
                visible: self.visible,
                lift: hovered && !item.active,
                right: !st.opens_left,
                ban: item.warning.as_ref().and_then(|w| w.card_outline_tone),
                glass: props.glass,
            },
        );
        let surface = painted.shown.then(|| start..next());
        (surface, painted.chips)
    }
}
