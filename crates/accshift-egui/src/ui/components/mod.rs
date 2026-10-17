//! One module per Svelte component, same name in snake_case
//! (`TitleBar.svelte` is `title_bar.rs`).
//!
//! A component is a plain function `fn x(ui, rect, &props) -> Vec<Action>`
//! (or a struct with `show`): it reads its props and the [`Vars`], keeps its
//! transient state (hover, transitions, focus) in egui memory under its own
//! ids, never touches the app state, and returns what the user asked for as
//! [`Action`]s, which the [`AppModel`] applies after the frame is drawn.
//!
//! [`Vars`]: crate::app::vars::Vars
//! [`Action`]: crate::app::Action
//! [`AppModel`]: crate::app::AppModel

pub mod account_card;
pub mod avatar_fade_in;
pub mod back_card;
pub mod breadcrumb;
pub mod bulk_edit;
pub mod card_extension;
pub mod command_palette;
pub mod context_menu;
pub mod controls;
pub mod dialog;
pub mod focus;
pub mod folder_card;
pub mod icon;
pub mod list_view;
pub mod onboarding;
pub mod platform_icons;
pub mod search_cancel;
pub mod section_header;
pub mod secure_overlays;
pub mod title_bar;
pub mod toast;
pub mod use_grid_layout;
pub mod view_toggle;

/// Where the right button went up this frame: Chromium fires `contextmenu`
/// there on Windows, even if the pointer moved again in the same frame.
pub fn context_click_pos(ui: &egui::Ui) -> Option<egui::Pos2> {
    ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                ..
            } => Some(*pos),
            _ => None,
        })
    })
}

fn click_eaten_id() -> egui::Id {
    egui::Id::unique("drag-eats-click")
}

/// `handleCaptureClick`: the drag manager swallows the click a drag release
/// fires, before any card sees it. Set for the release's frame only.
pub fn set_click_eaten(ctx: &egui::Context, eaten: bool) {
    ctx.data_mut(|d| d.insert_temp(click_eaten_id(), eaten));
}

/// A primary click on `response` that no drag release swallowed.
pub fn clicked(response: &egui::Response) -> bool {
    response.clicked()
        && !response
            .ctx
            .data(|d| d.get_temp::<bool>(click_eaten_id()))
            .unwrap_or(false)
}

/// `ghostEl`: a click-through layer over everything at `r`, scaled about its
/// centre, its contents faded to `opacity` as one group (a layer where the
/// renderer has them, each shape where it has not). The cards under it keep
/// the pointer.
pub fn drag_ghost(
    ctx: &egui::Context,
    r: egui::Rect,
    scale: f32,
    opacity: f32,
    add: impl FnOnce(&mut egui::Ui, egui::Rect),
) {
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost"));
    let r = r.translate(snap_offset(ctx, r.min));
    let t = egui::emath::TSTransform::new(r.center().to_vec2() * (1.0 - scale), scale);
    egui::Area::new(layer.id)
        .order(layer.order)
        .interactable(false)
        .movable(false)
        .constrain(false)
        .fixed_pos(r.min)
        .show(ctx, |area| {
            let mut gui = area.new_child(egui::UiBuilder::new().max_rect(r));
            gui.set_clip_rect(egui::Rect::EVERYTHING);
            let group = opacity < 1.0 && !cfg!(feature = "glow");
            if group {
                crate::ui::backdrop_filter::Mark::Group { opacity }.paint(gui.painter(), r);
            } else {
                gui.multiply_opacity(opacity);
            }
            let scope = TransformScope::begin(ctx, gui.layer_id(), t);
            add(&mut gui, r);
            scope.end(ctx, None);
            if group {
                crate::ui::backdrop_filter::Mark::End.paint(gui.painter(), r);
            }
        });
}

/// What a box under a scale moves by before it lays out its content:
/// Blink drops the subpixel part of its offset in layout units (the
/// transform could not keep it), so its corner `corner` sits on a whole
/// device pixel.
pub fn snap_offset(ctx: &egui::Context, corner: egui::Pos2) -> egui::Vec2 {
    let ppp = ctx.pixels_per_point();
    let device = (corner.to_vec2() * ppp * 64.0).round() / 64.0;
    (device.round() - device) / ppp
}

/// Shapes painted under a CSS `transform` (a scale about a point): the
/// boxes and meshes are mapped once painted, the text is laid out at its
/// transformed size as it is painted ([`crate::ui::text::layout`]). Scopes
/// nest.
#[must_use]
pub struct TransformScope {
    layer: egui::LayerId,
    from: usize,
    t: egui::emath::TSTransform,
    outer: Option<egui::emath::TSTransform>,
}

impl TransformScope {
    /// Starts painting under `t` on `layer`.
    pub fn begin(ctx: &egui::Context, layer: egui::LayerId, t: egui::emath::TSTransform) -> Self {
        let outer = crate::ui::text::transform(ctx);
        crate::ui::text::set_transform(ctx, Some(outer.map_or(t, |o| o * t)));
        Self {
            layer,
            from: shape_count(ctx, layer),
            t,
            outer,
        }
    }

    /// Maps what was painted since [`Self::begin`], clip rects included.
    /// `base`: the clip of the transformed element's container (a
    /// scroller's), which stays where it is and bounds the mapped ones.
    pub fn end(self, ctx: &egui::Context, base: Option<egui::Rect>) {
        crate::ui::text::set_transform(ctx, self.outer);
        let to = shape_count(ctx, self.layer);
        if to <= self.from {
            return;
        }
        ctx.graphics_mut(|g| {
            let list = g.entry(self.layer);
            let shapes: Vec<egui::epaint::ClippedShape> = list
                .all_entries()
                .skip(self.from)
                .take(to - self.from)
                .cloned()
                .collect();
            for (i, s) in shapes.into_iter().enumerate() {
                let clip = match base {
                    None => self.t.mul_rect(s.clip_rect),
                    Some(base) if s.clip_rect == base => base,
                    Some(base) => self.t.mul_rect(s.clip_rect).intersect(base),
                };
                let mut shape = s.shape;
                if !matches!(shape, egui::Shape::Text(_)) {
                    shape.transform(self.t);
                }
                list.set(egui::layers::ShapeIdx(self.from + i), clip, shape);
            }
        });
    }
}

/// Shapes on `layer` so far this frame.
pub fn shape_count(ctx: &egui::Context, layer: egui::LayerId) -> usize {
    ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0))
}

fn held_id() -> egui::Id {
    egui::Id::unique("css-active")
}

/// `:active`: the widget the primary button went down on stays pressed
/// until the button comes up, wherever the pointer went since. egui drops a
/// press past its click distance or duration. Run once per frame, before
/// the widgets.
pub fn track_held(ctx: &egui::Context) {
    let hit = ctx.viewport(|vp| vp.hits.click.map(|w| w.id));
    let mut held: Option<egui::Id> = ctx.data(|d| d.get_temp(held_id())).flatten();
    let mut pressed_or_released = false;
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } = event
            {
                held = if *pressed { hit } else { None };
                pressed_or_released = true;
            }
        }
    });
    ctx.data_mut(|d| {
        d.insert_temp(held_id(), held);
        if pressed_or_released {
            d.remove_temp::<bool>(dropped_id());
        }
    });
}

fn dropped_id() -> egui::Id {
    egui::Id::unique("css-active-dropped")
}

/// The DOM moved the pressed node: nothing matches `:active` until the
/// next press (Chromium hands the active chain to the node's parent).
pub fn drop_held(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(held_id(), None::<egui::Id>);
        d.insert_temp(dropped_id(), true);
    });
}

/// `response`'s widget matches `:active`.
pub fn held(response: &egui::Response) -> bool {
    let (stored, dropped) = response.ctx.data(|d| {
        (
            d.get_temp::<Option<egui::Id>>(held_id()).flatten(),
            d.get_temp::<bool>(dropped_id()).unwrap_or(false),
        )
    });
    !dropped && (response.is_pointer_button_down_on() || stored == Some(response.id))
}

/// `:hover`: Chromium keeps hovering what is under the pointer while a
/// button is held, where egui hovers nothing but the pressed widget.
pub fn css_hovered(response: &egui::Response) -> bool {
    response.hovered()
        || (response.enabled()
            && response
                .ctx
                .viewport(|vp| vp.interact_widgets.hovered.contains(&response.id)))
}
