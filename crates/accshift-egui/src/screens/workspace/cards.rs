//! The card pass: every laid out box is mounted and sensed, and painted
//! when it crosses the viewport; then the cards' shapes are restacked so an
//! open extension covers its neighbours.

use std::collections::HashMap;

use accshift_app::actions::avatar_fallback::{avatar_gradient, avatar_seed};
use accshift_app::actions::extension::{note_section, warning_section};
use accshift_app::display::{AccountCardItem, FolderCardItem, SectionHeaderItem, SectionLabel};
use accshift_app::folders::{FolderInfo, ItemKind, ItemRef};
use accshift_app::i18n::{translate, Locale};
use accshift_app::platform_add_flow::{PlatformAddFlow, SetupExtension};
use accshift_app::platforms::PlatformAccount;
use accshift_theme::ResolvedTheme;

use egui::layers::ShapeIdx;
use egui::{pos2, vec2, Context, Id, LayerId, Rect, Role, Sense, Ui, Vec2, WidgetInfo};

use super::ghost::GhostSources;
use super::layout::{Laid, Node, Placed};
use super::{Frame, Ghost, Workspace, PAD_TOP};
use crate::app::accounts::Cs2Cards;
use crate::app::avatars::{AvatarTexture, AvatarTextures};
use crate::app::vars::{rgba, Vars};
use crate::app::{Action, MenuTarget};
use crate::theme::Rgba;
use crate::ui::anim::{Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::account_card::{
    self, AccountCardProps, AvatarContent, AvatarPicture, CardEvent,
};
use crate::ui::components::avatar_fade_in;
use crate::ui::components::back_card::{self, BackCardEvent, BackCardProps};
use crate::ui::components::card_extension::ChipHit;
use crate::ui::components::folder_card::{self, entrance, FolderCardEvent, FolderCardProps};
use crate::ui::components::list_view::ListEvent;
use crate::ui::components::onboarding;
use crate::ui::components::section_header::{self, SectionHeaderEvent, SectionHeaderProps};
use crate::ui::components::TransformScope;
use crate::ui::css;
use crate::ui::text::{self, LineHeight, TextStyle};

/// How a dragged folder section fades and shrinks (140 ms ease out).
const SECTION_DRAG: Transition = Transition::new(0.14, Timing::EASE_OUT);

/// What a card or row event asks the app to do. The grid's cards speak the
/// list view's events, so both map to actions here.
pub(super) fn card_action(event: ListEvent) -> Action {
    match event {
        ListEvent::Back => Action::NavigateBack,
        ListEvent::Navigate(id) => Action::NavigateToFolder(Some(id)),
        ListEvent::FolderMenu(f, at) => Action::ContextMenu(MenuTarget::Folder(f), at),
        ListEvent::Activate(id) => Action::AccountActivate(id),
        ListEvent::Switch(id) => Action::AccountSwitch(id),
        ListEvent::AccountMenu(a, at) => Action::ContextMenu(MenuTarget::Account(*a), at),
        ListEvent::ToggleSection(key) => Action::ToggleSection(key),
    }
}

/// A card colour setting: none when empty, the accent for `"accent"` (bulk
/// edit), else a CSS colour.
pub(super) fn card_color(resolved: &ResolvedTheme, vars: &Vars, value: &str) -> Option<Rgba> {
    match value.trim() {
        "" => None,
        "accent" => Some(vars.accent),
        css => resolved.eval_color(css).map(rgba),
    }
}

/// A section header's folder id (none for the root section) and its label.
pub(super) fn header_label(h: &SectionHeaderItem, locale: Locale) -> (Option<&str>, String) {
    match &h.label {
        SectionLabel::Folder(f) => (Some(f.id.as_str()), f.name.clone()),
        SectionLabel::Root => (None, translate(locale, "list.rootSection")),
    }
}

/// One account card's shapes in the workspace's layer.
pub(super) struct Stack {
    pub range: std::ops::Range<usize>,
    pub surface: Option<std::ops::Range<usize>>,
    pub raised: bool,
}

pub(super) fn shape_count(ctx: &Context, layer: LayerId) -> usize {
    ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0))
}

/// Reorders the cards' shapes in `span` in three layers: the surfaces of
/// hidden extensions under every face, the card whose extension shows over
/// all. The order within each layer stays the paint order.
pub(super) fn restack(
    ctx: &Context,
    layer: LayerId,
    span: std::ops::Range<usize>,
    cards: &[Stack],
) {
    if !cards.iter().any(|c| c.surface.is_some()) {
        return;
    }
    let lo = span.start;
    let mut group = vec![1u8; span.len()];
    for c in cards {
        if c.raised {
            for i in c.range.clone() {
                group[i - lo] = 2;
            }
        } else if let Some(s) = &c.surface {
            for i in s.clone() {
                group[i - lo] = 0;
            }
        }
    }
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let mut shapes: Vec<Option<egui::epaint::ClippedShape>> = list
            .all_entries()
            .skip(lo)
            .take(span.len())
            .cloned()
            .map(Some)
            .collect();
        let mut at = lo;
        for z in 0..3u8 {
            for (i, shape) in shapes.iter_mut().enumerate() {
                if group[i] != z {
                    continue;
                }
                if let Some(s) = shape.take() {
                    list.set(ShapeIdx(at), s.clip_rect, s.shape);
                }
                at += 1;
            }
        }
    });
}

/// A folder section under a group opacity and a scale while it draws, its
/// nodes' keys starting with `prefix`.
struct OpenSection {
    prefix: String,
    /// Its [`crate::ui::components::snap_offset`]: its nodes lay out from
    /// there.
    snap: Vec2,
    /// The scale about the section's centre, when not 1.
    scope: Option<TransformScope>,
}

impl OpenSection {
    /// Lands the group and scales its shapes. The scroll clip stays where
    /// it is, as the scroller's own.
    fn close(self, ui: &Ui, clip: Rect) {
        Mark::End.paint(ui.painter(), clip);
        if let Some(scope) = self.scope {
            scope.end(ui.ctx(), Some(clip));
        }
    }
}

/// The add flow on this tab, as the cards see it.
pub(super) struct AddFlowCards<'a> {
    /// The account being set up: the other cards keep their extension shut.
    setup_id: Option<&'a str>,
    /// The card whose extension is forced open.
    forced_id: Option<&'a str>,
    /// The pending setup card and the setup sections that replace its own.
    setup: Option<(&'a str, &'a SetupExtension)>,
}

impl<'a> AddFlowCards<'a> {
    pub(super) fn new(
        add_flow: &'a PlatformAddFlow,
        tab: &str,
        setup: &'a Option<(PlatformAccount, SetupExtension)>,
    ) -> Self {
        let setup_id = add_flow
            .active_platform_setup_id(tab)
            .filter(|id| !id.is_empty());
        Self {
            setup_id,
            forced_id: setup_id.filter(|id| add_flow.is_forced_open(tab, id)),
            setup: setup
                .as_ref()
                .map(|(account, extension)| (account.id.as_str(), extension)),
        }
    }

    /// The setup sections of `account_id`, when it is the card being set
    /// up.
    fn setup_of(&self, account_id: &str) -> Option<&'a SetupExtension> {
        self.setup
            .filter(|(id, _)| *id == account_id)
            .map(|(_, extension)| extension)
    }
}

/// What the card pass reads besides the frame.
pub(super) struct CardsInput<'a> {
    pub avatars: &'a mut AvatarTextures,
    pub cs2: &'a Cs2Cards,
    pub add_flow: AddFlowCards<'a>,
}

/// The keyboard focus this frame, and the layout box of the card that has
/// it.
pub(super) struct Focus {
    pub id: Option<Id>,
    pub rect: Option<Rect>,
}

/// What the card pass leaves for the rest of the frame.
pub(super) struct CardsDrawn {
    pub focus: Focus,
    /// Each folder and account card's mount id.
    pub card_ids: HashMap<ItemRef, Id>,
    /// The back card on screen.
    pub back: Option<Rect>,
    pub ghosts: GhostSources,
}

/// Mounts, senses and paints every laid out box in order, pushing what
/// their events ask for.
pub(super) fn draw_cards(
    ui: &mut Ui,
    ws: &mut Workspace,
    f: &Frame,
    input: CardsInput,
    laid: &Laid,
    actions: &mut Vec<Action>,
) -> CardsDrawn {
    let ctx = &f.ctx;
    ws.visible_decoding = 0;
    #[cfg(test)]
    ws.layout.clear();
    let layer = ui.layer_id();
    let cards_from = shape_count(ctx, layer);
    let mut pass = CardPass {
        f,
        input,
        sections: &laid.sections,
        // The viewport in layout coordinates.
        view_rect: Rect::from_min_size(pos2(0.0, f.scroll - PAD_TOP), f.viewport.size()),
        avatar_px: (f.v.grid_card_avatar_size() * f.ppp).round() as u32,
        layer,
        kb_focus: ws.kb_focus.clone(),
        dragged_section: ws
            .ghost
            .as_ref()
            .filter(|g| f.drag.dragging && g.item.kind == ItemKind::Folder)
            .map(|g| g.item.id.clone()),
        open: None,
        stacks: Vec::new(),
        chips: Vec::new(),
        out: CardsDrawn {
            focus: Focus {
                id: ctx.memory(|m| m.focused()),
                rect: None,
            },
            card_ids: HashMap::new(),
            back: None,
            ghosts: GhostSources::default(),
        },
    };
    for p in &laid.placed {
        pass.place(ui, ws, p, actions);
    }
    crate::ui::anim::release(ctx);
    if let Some(open) = pass.open.take() {
        open.close(ui, f.clip);
    }
    restack(
        ctx,
        layer,
        cards_from..shape_count(ctx, layer),
        &pass.stacks,
    );
    // Clickable chips are sensed after every card, so a chip over a
    // neighbour takes the click.
    for hit in pass.chips {
        let response = ui
            .interact(hit.rect, hit.id, Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if crate::ui::components::clicked(&response) {
            actions.push(Action::SetupChip(hit.section, hit.chip));
        }
    }
    pass.out
}

/// The card pass's state while it walks the boxes.
struct CardPass<'p> {
    f: &'p Frame<'p>,
    input: CardsInput<'p>,
    /// The folder sections' boxes, layout coordinates.
    sections: &'p [(Option<String>, Rect)],
    view_rect: Rect,
    avatar_px: u32,
    layer: LayerId,
    kb_focus: Option<String>,
    /// The folder whose section is dragged: it fades and shrinks as one.
    dragged_section: Option<String>,
    open: Option<OpenSection>,
    stacks: Vec<Stack>,
    chips: Vec<ChipHit>,
    out: CardsDrawn,
}

impl CardPass<'_> {
    fn place(&mut self, ui: &mut Ui, ws: &mut Workspace, p: &Placed, actions: &mut Vec<Action>) {
        let f = self.f;
        let ctx = &f.ctx;
        // What the iteration writes before the mount is no card's.
        crate::ui::anim::release(ctx);
        if let Some(open) = self.open.take_if(|o| !p.key.starts_with(&o.prefix)) {
            open.close(ui, f.clip);
        }
        if let Node::Header(SectionHeaderItem {
            label: SectionLabel::Folder(folder),
            ..
        }) = &p.node
        {
            self.open_section(ui, p, folder);
        }
        #[cfg(test)]
        ws.layout.push((p.node.label(), p.rect));
        let id = ws.mount(&p.key, f.frame, f.now);
        crate::ui::anim::own_from(ctx, id);
        if let Some(item) = p.node.item_ref() {
            self.out.card_ids.insert(item, id);
        }
        if matches!(p.node, Node::Back) {
            self.out.back = Some(f.to_screen(p.rect));
        }
        let visible = p.visible_in(self.view_rect);
        let snap = self.open.as_ref().map_or(Vec2::ZERO, |o| o.snap);
        let screen = f.to_screen(p.rect).translate(snap);
        if matches!(p.node, Node::Account(_)) {
            // The onboarding tour points at the first account card.
            onboarding::record_target(ctx, |t| {
                t.account_card.get_or_insert(screen);
            });
        }
        if self.out.focus.id == Some(id) {
            self.out.focus.rect = Some(p.rect);
        }
        let avatar = match &p.node {
            Node::Account(a) => Some(self.avatar(ws, &p.key, a, id, visible)),
            _ => None,
        };
        if let Some(avatar) = &avatar {
            self.keep_for_ghosts(ws.ghost.as_ref(), p, avatar);
        }
        if !visible {
            self.sense_hidden(ui, p, screen, id, avatar.as_ref());
            return;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(screen));
        let child_clip = match p.clip {
            Some(c) => f.to_screen(c).translate(snap).intersect(f.clip),
            None => f.clip,
        };
        child.set_clip_rect(child_clip);
        child.multiply_opacity(p.opacity);
        let events = match &p.node {
            Node::Back => self.draw_back(&mut child, screen, id),
            Node::EmptyFolder => {
                draw_empty_folder(&child, screen, id, f);
                Vec::new()
            }
            Node::Header(h) => self.draw_header(&mut child, screen, id, h),
            Node::Folder(folder) => self.draw_folder(&mut child, screen, id, folder),
            Node::Account(a) => self.draw_account(&mut child, screen, id, a, avatar),
        };
        actions.extend(events.into_iter().map(card_action));
    }

    /// Opens the group of a folder section at its header: while dragged,
    /// the section fades and shrinks as one, and its key prefix is kept for
    /// the section ghost.
    fn open_section(&mut self, ui: &Ui, p: &Placed, folder: &FolderInfo) {
        let f = self.f;
        let ctx = &f.ctx;
        let prefix = p.key.strip_suffix("header").unwrap_or(&p.key).to_string();
        let dragged = self.dragged_section.as_deref() == Some(folder.id.as_str());
        if dragged {
            self.out.ghosts.section = Some(prefix.clone());
        }
        let sid = Id::unique(("section-dragged", &folder.id));
        let (to_opacity, to_scale) = if dragged { (0.35, 0.97) } else { (1.0, 1.0) };
        let opacity = crate::ui::anim::transition(ctx, sid.with("o"), to_opacity, &SECTION_DRAG);
        let scale = crate::ui::anim::transition(ctx, sid.with("s"), to_scale, &SECTION_DRAG);
        let changed = opacity < 1.0 || scale < 1.0;
        if !changed {
            return;
        }
        let section = self
            .sections
            .iter()
            .find(|(id, _)| id.as_deref() == Some(folder.id.as_str()))
            .map_or(f.to_screen(p.rect), |(_, r)| f.to_screen(*r));
        Mark::Group { opacity }.paint(ui.painter(), f.clip);
        let scaled = scale != 1.0;
        let snap = if scaled {
            crate::ui::components::snap_offset(ctx, section.min)
        } else {
            Vec2::ZERO
        };
        self.open = Some(OpenSection {
            prefix,
            snap,
            scope: scaled.then(|| {
                TransformScope::begin(
                    ctx,
                    self.layer,
                    egui::emath::TSTransform::new(
                        (section.center() + snap).to_vec2() * (1.0 - scale),
                        scale,
                    ),
                )
            }),
        });
    }

    /// An account card's avatar, its decode started; blurred as well when
    /// the card shows and wants it.
    fn avatar(
        &mut self,
        ws: &mut Workspace,
        key: &str,
        a: &AccountCardItem,
        id: Id,
        visible: bool,
    ) -> AvatarContent {
        let f = self.f;
        let avatars = &mut *self.input.avatars;
        let mut content = ws.account_avatar(avatars, f, key, a, self.avatar_px, visible);
        if let (AvatarContent::Picture(picture), Some(url)) =
            (&mut content, a.avatar.url.as_deref())
        {
            if visible && account_card::wants_blur(&f.ctx, id, a) {
                // A 6 px blur, in device pixels.
                picture.blurred = avatars.blurred(&f.ctx, url, self.avatar_px, 6.0 * f.ppp);
            }
        }
        content
    }

    /// Keeps an account card's avatar for the ghost that clones it: the
    /// dragged card's, or a card's in the dragged section.
    fn keep_for_ghosts(&mut self, ghost: Option<&Ghost>, p: &Placed, avatar: &AvatarContent) {
        let ghosts = &mut self.out.ghosts;
        if ghost.is_some_and(|g| p.node.shows(&g.item)) {
            ghosts.avatar = Some(avatar.clone());
        }
        if ghosts
            .section
            .as_ref()
            .is_some_and(|prefix| p.key.starts_with(prefix.as_str()))
        {
            ghosts.section_avatars.insert(p.key.clone(), avatar.clone());
        }
    }

    /// A box off screen is sensed, not painted, so Tab still walks it; its
    /// entrance and avatar fade clocks run as if it were on screen. The
    /// empty-folder message is skipped.
    fn sense_hidden(
        &self,
        ui: &mut Ui,
        p: &Placed,
        screen: Rect,
        id: Id,
        avatar: Option<&AvatarContent>,
    ) {
        let f = self.f;
        let label = match &p.node {
            Node::Back => translate(f.locale, "common.back"),
            Node::EmptyFolder => return,
            Node::Header(h) => header_label(h, f.locale).1,
            Node::Folder(folder) => folder.folder.name.clone(),
            Node::Account(a) => a.name.clone(),
        };
        let _sense = crate::bench::span("card_sense");
        sense_only(ui, screen, id, &label);
        if !matches!(p.node, Node::Header(_)) {
            entrance(ui, id, f.v);
        }
        if let Some(avatar) = avatar {
            let revealed = !matches!(avatar, AvatarContent::Decoding);
            avatar_fade_in::avatar_opacity(&f.ctx, id, revealed, f.v);
        }
    }

    fn draw_back(&self, ui: &mut Ui, screen: Rect, id: Id) -> Vec<ListEvent> {
        let f = self.f;
        let props = BackCardProps {
            vars: f.v,
            id,
            locale: f.locale,
            accent: f.accent,
            drag_over: f.drag.over_back,
            focus_visible: f.focus_visible,
            grid_dragging: f.drag.dragging,
        };
        back_card::back_card(ui, screen, &props)
            .into_iter()
            .map(|BackCardEvent::Back| ListEvent::Back)
            .collect()
    }

    fn draw_header(
        &self,
        ui: &mut Ui,
        screen: Rect,
        id: Id,
        h: &SectionHeaderItem,
    ) -> Vec<ListEvent> {
        let f = self.f;
        let (folder, label) = header_label(h, f.locale);
        let props = SectionHeaderProps {
            vars: f.v,
            id,
            folder,
            label: &label,
            count: h.count,
            card_color: card_color(f.resolved, f.v, &h.card_color),
            accent: f.accent,
            collapsed: h.collapsed,
            focus_visible: f.focus_visible,
        };
        section_header::section_header(ui, screen, &props)
            .into_iter()
            .filter_map(|event| match event {
                SectionHeaderEvent::Toggle => {
                    Some(ListEvent::ToggleSection(h.collapse_key.clone()))
                }
                SectionHeaderEvent::Navigate(id) => Some(ListEvent::Navigate(id)),
                SectionHeaderEvent::ContextMenu(at) => match &h.label {
                    SectionLabel::Folder(folder) => Some(ListEvent::FolderMenu(folder.clone(), at)),
                    SectionLabel::Root => None,
                },
            })
            .collect()
    }

    fn draw_folder(
        &self,
        ui: &mut Ui,
        screen: Rect,
        id: Id,
        item: &FolderCardItem,
    ) -> Vec<ListEvent> {
        let f = self.f;
        let folder = &item.folder;
        let props = FolderCardProps {
            vars: f.v,
            id,
            name: &folder.name,
            card_color: card_color(f.resolved, f.v, &item.card_color),
            accent: f.accent,
            drag_over: f.drag.over_folder.as_deref() == Some(folder.id.as_str()),
            dragged: item.dragged,
            focus_visible: f.focus_visible,
            kb_focus: self.kb_focus.as_deref() == Some(folder.id.as_str()),
            grid_dragging: f.drag.dragging,
            ghost: false,
        };
        folder_card::folder_card(ui, screen, &props)
            .into_iter()
            .map(|event| match event {
                FolderCardEvent::Open => ListEvent::Navigate(folder.id.clone()),
                FolderCardEvent::ContextMenu(at) => ListEvent::FolderMenu(folder.clone(), at),
            })
            .collect()
    }

    fn draw_account(
        &mut self,
        ui: &mut Ui,
        screen: Rect,
        id: Id,
        a: &AccountCardItem,
        avatar: Option<AvatarContent>,
    ) -> Vec<ListEvent> {
        let f = self.f;
        let account_id = a.account.id.as_str();
        let cs2 = self.input.cs2.card(account_id);
        let flow = &self.input.add_flow;
        let setup = flow.setup_of(account_id);
        // A card being set up shows the setup sections instead of its own:
        // the warning, the note, then the extra sections (the CS2 row).
        let sections: Vec<_> = match setup {
            Some(extension) => extension.sections.clone(),
            None => warning_section(a.warning.as_ref(), f.locale)
                .into_iter()
                .chain(note_section(&a.note, a.show_note_inline))
                .chain(cs2.iter().flat_map(|c| c.sections.iter().cloned()))
                .collect(),
        };
        let clickable: Vec<(usize, usize)> = setup
            .map(|extension| extension.chip_actions.iter().map(|(at, _)| *at).collect())
            .unwrap_or_default();
        // No username badge in bulk edit (one click switches) or on a card
        // being set up.
        let badge = cs2
            .and_then(|c| c.badge.as_ref())
            .filter(|_| !a.single_click_switch && !a.pending_setup);
        let start = shape_count(&f.ctx, self.layer);
        let _paint = crate::bench::span("card_paint");
        crate::bench::mark_once("cards");
        let out = account_card::account_card(
            ui,
            screen,
            &AccountCardProps {
                vars: f.v,
                id,
                item: a,
                card_color: card_color(f.resolved, f.v, &a.card_color),
                avatar: avatar.unwrap_or(AvatarContent::Loading),
                focus_visible: f.focus_visible,
                locale: f.locale,
                time: &f.time,
                badge,
                extension: &sections,
                extension_disabled: a.single_click_switch || f.drag.dragging,
                extension_forced: !a.single_click_switch && flow.forced_id == Some(account_id),
                extension_hover_disabled: a.single_click_switch
                    || flow.setup_id.is_some_and(|id| id != account_id),
                clickable_chips: &clickable,
                glass: f.glass,
                kb_focus: self.kb_focus.as_deref() == Some(account_id),
                grid_dragging: f.drag.dragging,
                ghost: false,
            },
        );
        self.stacks.push(Stack {
            range: start..shape_count(&f.ctx, self.layer),
            surface: out.surface,
            raised: out.raised,
        });
        self.chips.extend(out.chips);
        out.events
            .into_iter()
            .map(|event| match event {
                CardEvent::Activate => ListEvent::Activate(a.account.id.clone()),
                CardEvent::Switch => ListEvent::Switch(a.account.id.clone()),
                CardEvent::ContextMenu(at) => {
                    ListEvent::AccountMenu(Box::new(a.account.clone()), at)
                }
            })
            .collect()
    }
}

/// The message under the back card of an empty folder: 12 px muted text
/// 2 px in, fading in with the card entrance.
fn draw_empty_folder(ui: &Ui, screen: Rect, id: Id, f: &Frame) {
    let v = f.v;
    let (o, _) = entrance(ui, id, v);
    let style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_muted.alpha(v.fg_muted.a * o).to_color32());
    let r = screen.translate(vec2(2.0, 2.0));
    text::paint(
        ui.painter(),
        r,
        &translate(f.locale, "app.folderEmpty"),
        &style,
    );
}

/// Senses a card that is not painted, so Tab still walks it.
fn sense_only(ui: &mut Ui, rect: Rect, id: Id, label: &str) {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    ui.ctx()
        .accesskit_node_builder(id, |node| node.set_role(egui::accesskit::Role::Button));
}

impl Workspace {
    /// The avatar box content of an account card, starting its decode.
    pub(super) fn account_avatar(
        &mut self,
        avatars: &mut AvatarTextures,
        f: &Frame,
        key: &str,
        a: &AccountCardItem,
        px: u32,
        visible: bool,
    ) -> AvatarContent {
        let picture = a
            .avatar
            .url
            .as_deref()
            .map(|url| avatars.get(f.jobs, url, px));
        match picture {
            Some(AvatarTexture::Ready { texture, size }) => {
                if self.decoding.remove(key) {
                    self.last_reveal = f.now;
                }
                AvatarContent::Picture(AvatarPicture {
                    texture,
                    size,
                    blurred: None,
                })
            }
            Some(AvatarTexture::Pending) => {
                self.decoding.insert(key.to_string());
                if visible {
                    self.visible_decoding += 1;
                }
                AvatarContent::Decoding
            }
            Some(AvatarTexture::Failed) | None => {
                self.decoding.remove(key);
                if a.avatar.loading {
                    AvatarContent::Loading
                } else {
                    self.initials(f.resolved, a)
                }
            }
        }
    }

    /// The initials avatar's background colour and gradient, picked from
    /// the account's name and id and cached per hue pair.
    fn initials(&mut self, resolved: &ResolvedTheme, item: &AccountCardItem) -> AvatarContent {
        let account = &item.account;
        let g = avatar_gradient(&avatar_seed(
            &account.display_name,
            &account.username,
            &account.id,
        ));
        let (background, gradient) = self
            .gradients
            .entry((g.hue, g.hue_fade))
            .or_insert_with(|| {
                let hsl = |h: u16, s: u8, l: u8| {
                    resolved
                        .eval_color(&format!("hsl({h} {s}% {l}%)"))
                        .map(rgba)
                        .unwrap_or(Rgba::TRANSPARENT)
                };
                let css_rgba = |c: Rgba| format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a);
                let gradient = css::gradient(&format!(
                    "linear-gradient(145deg, {}, {})",
                    css_rgba(hsl(g.hue, 80, 56)),
                    css_rgba(hsl(g.hue_fade, 66, 38)),
                ));
                (hsl(g.hue, 72, 43), gradient)
            })
            .clone();
        AvatarContent::Initials {
            background,
            gradient,
        }
    }
}
