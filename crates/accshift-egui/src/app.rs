//! The main screen: title bar, platform tabs, header, account grid.
//!
//! Everything is painted by hand at the metrics of the Svelte shell
//! (`TitleBar.svelte`, `AccountCard.svelte`, `FolderCard.svelte`,
//! `ViewToggle.svelte`, the `--grid-card-*` tokens of `app.css`), so the two
//! can be laid side by side.

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use egui::epaint::{RectShape, TextShape};
use egui::{
    pos2, vec2, Align2, Color32, ColorImage, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui,
    Vec2,
};

use crate::avatars::{self, Decoded};
use crate::bench;
use crate::data::{self, Dataset, Item, Platform};
use crate::icons;
use crate::theme::{ease_out, fallback_gradient, Rgba, Theme, GLASS_DARK};

const TITLE_H: f32 = 36.0;
const GRID_TOP: f32 = 84.0;
const CARD_W: f32 = 100.0;
const CARD_H: f32 = 136.0;
const CARD_PAD: f32 = 8.0;
const CARD_GAP: f32 = 10.0;
const AVATAR: f32 = 68.0;
const SCROLL_GUTTER: f32 = 10.0;
const SWITCH_DELAY: f64 = 0.9;
/// Avatars are rasterized once at this size: 68 pt at 2x, plus the 1.04 hover zoom.
pub const AVATAR_PX: u32 = 160;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Personas,
    Platform(Platform),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Grid,
    List,
}

#[derive(Default, Clone, Copy)]
pub struct Options {
    /// Paint the recorder's stand-in desktop under the window, for offscreen
    /// captures where no OS backdrop exists.
    pub desktop_backdrop: bool,
}

struct AvatarTex {
    sharp: TextureHandle,
    blurred: TextureHandle,
    loaded_at: f64,
}

pub struct AccshiftApp {
    theme: &'static Theme,
    data: Dataset,
    tab: Tab,
    folder: Option<&'static str>,
    view: ViewMode,
    search: String,
    armed: Option<String>,
    switching: Option<(String, f64)>,
    page_key: (Tab, Option<&'static str>, ViewMode),
    page_since: f64,
    icons: HashMap<(usize, u32), TextureHandle>,
    avatars: Vec<Option<AvatarTex>>,
    avatar_rx: Option<Receiver<Decoded>>,
    gradients: HashMap<String, TextureHandle>,
    backdrop: Option<TextureHandle>,
    frame_index: u64,
    ready_marked: bool,
    all_avatars_drawn_at: Option<u64>,
}

impl AccshiftApp {
    pub fn new(ctx: &egui::Context, options: Options) -> Self {
        install_fonts(ctx);
        let avatar_rx = Some(avatars::spawn(ctx.clone(), AVATAR_PX));
        let backdrop = options.desktop_backdrop.then(|| {
            ctx.load_texture(
                "desktop",
                crate::backdrop::render(860, 440, 2.0),
                TextureOptions::LINEAR,
            )
        });
        Self {
            theme: &GLASS_DARK,
            data: data::demo(),
            tab: Tab::Platform(Platform::Steam),
            folder: None,
            view: ViewMode::Grid,
            search: String::new(),
            armed: None,
            switching: None,
            page_key: (Tab::Platform(Platform::Steam), None, ViewMode::Grid),
            page_since: 0.0,
            icons: HashMap::new(),
            avatars: (0..data::AVATARS.len()).map(|_| None).collect(),
            avatar_rx,
            gradients: HashMap::new(),
            backdrop,
            frame_index: 0,
            ready_marked: false,
            all_avatars_drawn_at: None,
        }
    }

    /// Every avatar texture has been uploaded (the offscreen captures wait
    /// for this before taking a frame).
    pub fn avatars_ready(&self) -> bool {
        self.avatar_rx.is_none()
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if self.frame_index == 1 {
            // Frame 0 has been painted and swapped by now.
            bench::mark("first_frame");
        }
        if let (Some(at), false) = (self.all_avatars_drawn_at, self.ready_marked) {
            if self.frame_index > at {
                bench::mark("ready");
                self.ready_marked = true;
            }
        }

        self.receive_avatars(&ctx, now);
        self.finish_switch(now);

        let key = (self.tab, self.folder, self.view);
        if key != self.page_key {
            self.page_key = key;
            self.page_since = now;
        }

        let full = ui.max_rect();
        let painter = ui.painter().clone();
        if let Some(desk) = &self.backdrop {
            painter.image(
                desk.id(),
                full,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.rect_filled(
            full,
            CornerRadius::ZERO,
            self.theme.window_fill.to_color32(),
        );

        self.title_bar(ui, full, now);
        match self.tab {
            Tab::Personas => self.personas_page(ui, full),
            Tab::Platform(platform) => self.platform_page(ui, full, platform, now),
        }

        // A press anywhere but on the armed card disarms it, like the
        // document mousedown listener of AccountCard.svelte.
        if self.armed.is_some() && ctx.input(|i| i.pointer.any_pressed()) {
            let over_armed = ctx
                .read_response(Id::new(("card", self.armed.clone().unwrap_or_default())))
                .is_some_and(|r| r.hovered());
            if !over_armed {
                self.armed = None;
            }
        }

        if self.frame_index == 0 {
            bench::mark("ui_done");
            ctx.request_repaint();
        }
        if self.all_avatars_drawn_at.is_some() && !self.ready_marked {
            ctx.request_repaint();
        }
        self.frame_index += 1;
    }

    // ------------------------------------------------------------- loading

    fn receive_avatars(&mut self, ctx: &egui::Context, now: f64) {
        let Some(rx) = &self.avatar_rx else { return };
        while let Ok(decoded) = rx.try_recv() {
            let sharp = ctx.load_texture(
                format!("avatar-{}", decoded.index),
                decoded.sharp,
                TextureOptions::LINEAR,
            );
            let blurred = ctx.load_texture(
                format!("avatar-blur-{}", decoded.index),
                decoded.blurred,
                TextureOptions::LINEAR,
            );
            self.avatars[decoded.index] = Some(AvatarTex {
                sharp,
                blurred,
                loaded_at: now,
            });
        }
        if self.avatars.iter().all(Option::is_some) {
            self.avatar_rx = None;
            self.all_avatars_drawn_at.get_or_insert(self.frame_index);
        }
    }

    fn finish_switch(&mut self, now: f64) {
        if let Some((id, since)) = &self.switching {
            if now - since >= SWITCH_DELAY {
                let platform = match self.tab {
                    Tab::Platform(p) => p,
                    Tab::Personas => Platform::Steam,
                };
                self.data.set_active(platform, id.clone());
                self.switching = None;
            }
        }
    }

    fn icon(&mut self, ctx: &egui::Context, svg: &'static [u8], size: f32) -> TextureHandle {
        let ppp = ctx.pixels_per_point();
        let px = (size * ppp).round().max(1.0) as u32;
        let key = (svg.as_ptr() as usize, px);
        self.icons
            .entry(key)
            .or_insert_with(|| {
                let image = avatars::raster(svg, px)
                    .unwrap_or_else(|_| ColorImage::filled([1, 1], Color32::TRANSPARENT));
                ctx.load_texture(format!("icon-{px}-{key:?}"), image, TextureOptions::LINEAR)
            })
            .clone()
    }

    fn paint_icon(&mut self, ui: &Ui, svg: &'static [u8], center: Pos2, size: f32, color: Rgba) {
        let tex = self.icon(ui.ctx(), svg, size);
        let rect = Rect::from_center_size(center, Vec2::splat(size));
        ui.painter().image(
            tex.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            color.to_color32(),
        );
    }

    fn gradient(&mut self, ctx: &egui::Context, seed: &str) -> TextureHandle {
        self.gradients
            .entry(seed.to_string())
            .or_insert_with(|| {
                let (a, b) = fallback_gradient(seed);
                ctx.load_texture(
                    format!("grad-{seed}"),
                    gradient_image(a, b, 48),
                    TextureOptions::LINEAR,
                )
            })
            .clone()
    }

    // ----------------------------------------------------------- title bar

    fn title_bar(&mut self, ui: &mut Ui, full: Rect, now: f64) {
        let t = self.theme;
        let bar = Rect::from_min_size(full.min, vec2(full.width(), TITLE_H));
        let drag = ui.interact(bar, Id::new("titlebar"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            Stroke::new(1.0, t.card.to_color32()),
        );

        // Left actions: 26x26 buttons from x = 10, 6 apart.
        let actions: [(&'static [u8], &str); 4] = [
            (icons::REFRESH, "Refresh"),
            (icons::PLUS, "Add account"),
            (icons::SETTINGS, "Settings"),
            (icons::BULK_EDIT, "Bulk edit"),
        ];
        for (i, (svg, label)) in actions.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(full.left() + 10.0 + i as f32 * 32.0, full.top() + 5.0),
                Vec2::splat(26.0),
            );
            let resp = button(ui, Id::new(("action", i)), rect, label);
            let hover = anim(ui, resp.id, resp.hovered(), 0.12);
            let bg = Rgba::TRANSPARENT.lerp(t.muted, hover);
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), bg.to_color32());
            let color = t.fg_muted.lerp(t.fg, hover);
            self.paint_icon(ui, svg, rect.center(), 14.0, color);
        }

        // Centered tabs: 32x28, 2 apart.
        let tabs = [
            Tab::Personas,
            Tab::Platform(Platform::Steam),
            Tab::Platform(Platform::Riot),
            Tab::Platform(Platform::Roblox),
        ];
        let total = tabs.len() as f32 * 32.0 + (tabs.len() as f32 - 1.0) * 2.0;
        let x0 = full.center().x - total / 2.0;
        for (i, tab) in tabs.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(x0 + i as f32 * 34.0, full.top() + 4.0),
                vec2(32.0, 28.0),
            );
            let (svg, accent, label) = match tab {
                Tab::Personas => (icons::PERSONAS, Rgba::hex(0xa855f7, 1.0), "Personas"),
                Tab::Platform(p) => (
                    match p {
                        Platform::Steam => icons::STEAM,
                        Platform::Riot => icons::RIOT,
                        Platform::Roblox => icons::ROBLOX,
                    },
                    Rgba::hex(p.accent(), 1.0),
                    p.name(),
                ),
            };
            let resp = button(ui, Id::new(("tab", i)), rect, label);
            if resp.clicked() && self.tab != tab {
                self.tab = tab;
                self.folder = None;
                self.search.clear();
                self.armed = None;
            }
            let active = self.tab == tab;
            let hover = anim(ui, resp.id, resp.hovered(), 0.12);
            let on = anim(ui, resp.id.with("on"), active, 0.12);
            let bg = Rgba::TRANSPARENT.lerp(t.card, hover.max(on));
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), bg.to_color32());
            let idle = t.fg_subtle.lerp(t.fg_muted, hover);
            self.paint_icon(ui, svg, rect.center(), 16.0, idle.lerp(accent, on));
        }

        // Caption strip: 46 px buttons, full height, flush right.
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let caption: [(&'static [u8], &str); 3] = [
            (icons::MINIMIZE, "Minimize"),
            (
                if maximized {
                    icons::RESTORE
                } else {
                    icons::MAXIMIZE
                },
                "Maximize",
            ),
            (icons::CLOSE, "Close"),
        ];
        for (i, (svg, label)) in caption.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(full.right() - 46.0 * (3 - i) as f32, full.top()),
                vec2(46.0, TITLE_H - 1.0),
            );
            let resp = button(ui, Id::new(("caption", i)), rect, label);
            let hover = anim(ui, resp.id, resp.hovered(), 0.12);
            let (bg, fg) = if i == 2 {
                (t.danger, Rgba::hex(0xffffff, 1.0))
            } else {
                (t.muted, t.fg)
            };
            ui.painter().rect_filled(
                rect,
                CornerRadius::ZERO,
                Rgba::TRANSPARENT.lerp(bg, hover).to_color32(),
            );
            self.paint_icon(ui, svg, rect.center(), 12.0, t.fg_muted.lerp(fg, hover));
            if resp.clicked() {
                let cmd = match i {
                    0 => egui::ViewportCommand::Minimized(true),
                    1 => egui::ViewportCommand::Maximized(!maximized),
                    _ => egui::ViewportCommand::Close,
                };
                ui.ctx().send_viewport_cmd(cmd);
            }
        }
        let _ = now;
    }

    // --------------------------------------------------------------- pages

    fn personas_page(&mut self, ui: &mut Ui, full: Rect) {
        let t = self.theme;
        let painter = ui.painter();
        painter.text(
            pos2(full.center().x, full.center().y - 8.0),
            Align2::CENTER_CENTER,
            "Personas",
            FontId::new(15.0, semibold()),
            t.fg.to_color32(),
        );
        painter.text(
            pos2(full.center().x, full.center().y + 14.0),
            Align2::CENTER_CENTER,
            "Not part of the spike.",
            FontId::proportional(12.0),
            t.fg_subtle.to_color32(),
        );
    }

    fn platform_page(&mut self, ui: &mut Ui, full: Rect, platform: Platform, now: f64) {
        let t = self.theme;
        let accent = Rgba::hex(platform.accent(), 1.0);

        // Header: section title or breadcrumb on the left, search and view
        // toggle on the right.
        let header_y = full.top() + 60.0;
        let crumb_font = |current: bool| {
            if current {
                FontId::new(12.0, semibold())
            } else {
                FontId::proportional(12.0)
            }
        };
        let at_root = self.folder.is_none();
        let root_color = if at_root { accent } else { t.fg_subtle };
        let crumb = ui.painter().layout_no_wrap(
            platform.name().to_string(),
            crumb_font(at_root),
            root_color.to_color32(),
        );
        let crumb_rect = Rect::from_min_size(
            pos2(full.left() + 16.0, header_y - crumb.size().y / 2.0 - 2.0),
            crumb.size() + vec2(8.0, 4.0),
        );
        let crumb_resp = ui.interact(crumb_rect, Id::new("crumb-root"), Sense::click());
        if crumb_resp.clicked() {
            self.folder = None;
        }
        ui.painter().galley(
            crumb_rect.min + vec2(4.0, 2.0),
            crumb,
            root_color.to_color32(),
        );
        let mut x = crumb_rect.right() + 4.0;
        if let Some(folder) = self.folder {
            let name = match folder {
                "demo-folder-smurfs" => "Smurfs",
                other => other,
            };
            let sep = ui.painter().layout_no_wrap(
                "/".into(),
                FontId::proportional(11.0),
                t.elevated.to_color32(),
            );
            ui.painter().galley(
                pos2(x, header_y - sep.size().y / 2.0),
                sep.clone(),
                t.elevated.to_color32(),
            );
            x += sep.size().x + 4.0;
            let g = ui
                .painter()
                .layout_no_wrap(name.into(), crumb_font(true), accent.to_color32());
            ui.painter().galley(
                pos2(x + 4.0, header_y - g.size().y / 2.0),
                g,
                accent.to_color32(),
            );
        }

        let right = full.right() - SCROLL_GUTTER - 17.0;
        let toggle = Rect::from_min_size(pos2(right - 66.0, header_y - 15.0), vec2(66.0, 30.0));
        self.view_toggle(ui, toggle);
        let search = Rect::from_min_size(
            pos2(toggle.left() - 8.0 - 240.0, header_y - 15.0),
            vec2(240.0, 30.0),
        );
        self.search_box(ui, search);

        let body = Rect::from_min_max(pos2(full.left(), full.top() + GRID_TOP - 8.0), full.max);
        let items = self.visible_items(platform);
        let entrance = ease_out(((now - self.page_since) / 0.26) as f32);
        if entrance < 1.0 {
            ui.ctx().request_repaint();
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical()
            .id_salt((
                "grid",
                self.page_key.0 == Tab::Platform(platform),
                self.folder,
            ))
            .auto_shrink([false, false])
            .show(&mut child, |ui| match self.view {
                ViewMode::Grid => self.grid(ui, platform, &items, entrance, now),
                ViewMode::List => self.list(ui, platform, &items, entrance),
            });
    }

    fn visible_items(&self, platform: Platform) -> Vec<Item> {
        let query = self.search.trim().to_lowercase();
        let accounts = self.data.accounts(platform);
        if !query.is_empty() {
            // Search flattens folders, like the web grid.
            return accounts
                .iter()
                .enumerate()
                .filter(|(_, a)| a.name.to_lowercase().contains(&query))
                .map(|(i, _)| Item::Account(i))
                .collect();
        }
        match (platform, self.folder) {
            (Platform::Steam, None) => self.data.steam_root.clone(),
            (Platform::Steam, Some(_)) => {
                let mut items = vec![Item::Folder {
                    id: "..",
                    name: "Back",
                }];
                items.extend(self.data.smurfs.iter().cloned());
                items
            }
            _ => (0..accounts.len()).map(Item::Account).collect(),
        }
    }

    fn view_toggle(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.theme;
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), t.muted.to_color32());
        for (i, (mode, svg, label)) in [
            (ViewMode::Grid, icons::GRID, "Grid view"),
            (ViewMode::List, icons::LIST, "List view"),
        ]
        .into_iter()
        .enumerate()
        {
            let b = Rect::from_min_size(
                rect.min + vec2(2.0 + i as f32 * 32.0, 2.0),
                vec2(30.0, 26.0),
            );
            let resp = button(ui, Id::new(("view", i)), b, label);
            if resp.clicked() {
                self.view = mode;
            }
            let on = anim(ui, resp.id.with("on"), self.view == mode, 0.1);
            let hover = anim(ui, resp.id, resp.hovered(), 0.1);
            ui.painter().rect_filled(
                b,
                CornerRadius::same(6),
                Rgba::TRANSPARENT.lerp(t.card, on).to_color32(),
            );
            let color = t.fg_muted.lerp(t.fg, on.max(hover));
            self.paint_icon(ui, svg, b.center(), 14.0, color);
        }
    }

    fn search_box(&mut self, ui: &mut Ui, rect: Rect) {
        let t = self.theme;
        let id = Id::new("search");
        let focused = ui.ctx().memory(|m| m.has_focus(id));
        let border = if focused {
            t.fg.mix(0.35, t.border)
        } else {
            t.border
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(8),
            t.card.to_color32(),
            Stroke::new(1.0, border.to_color32()),
            StrokeKind::Inside,
        );
        let edit = egui::TextEdit::singleline(&mut self.search)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(FontId::proportional(12.0))
            .text_color(t.fg.to_color32())
            .hint_text(
                egui::RichText::new("Search account...")
                    .size(12.0)
                    .color(t.fg_subtle.mix(0.55, Rgba::TRANSPARENT).to_color32()),
            )
            .margin(egui::Margin::symmetric(0, 0))
            .desired_width(rect.width() - 20.0 - 22.0);
        let inner = Rect::from_min_max(
            rect.min + vec2(10.0, 7.0),
            rect.max - vec2(10.0 + 22.0, 7.0),
        );
        let resp = ui.put(inner, edit);
        if resp.changed() {
            self.armed = None;
        }
        // Clear button, only with a query (the web field's X).
        if !self.search.is_empty() {
            let b = Rect::from_center_size(
                pos2(rect.right() - 18.0, rect.center().y),
                Vec2::splat(20.0),
            );
            let clear = button(ui, Id::new("search-clear"), b, "Clear search");
            let hover = anim(ui, clear.id, clear.hovered(), 0.1);
            self.paint_icon(
                ui,
                icons::CLOSE,
                b.center(),
                11.0,
                t.fg_muted.lerp(t.fg, hover.max(0.7)),
            );
            if clear.clicked() {
                self.search.clear();
            }
        }
    }

    // ---------------------------------------------------------------- grid

    fn grid(&mut self, ui: &mut Ui, platform: Platform, items: &[Item], entrance: f32, now: f64) {
        let width = ui.available_width();
        let usable = width - SCROLL_GUTTER;
        let cols = (((usable + CARD_GAP) / (CARD_W + CARD_GAP)).floor() as usize).max(1);
        let grid_w = cols as f32 * CARD_W + (cols as f32 - 1.0) * CARD_GAP;
        let rows = items.len().div_ceil(cols);
        let height = 8.0 + rows as f32 * (CARD_H + CARD_GAP) + 8.0;
        let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let x0 = area.left() + ((usable - grid_w) / 2.0).round();
        let y0 = area.top() + 8.0;
        for (i, item) in items.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let rect = Rect::from_min_size(
                pos2(
                    x0 + col as f32 * (CARD_W + CARD_GAP),
                    y0 + row as f32 * (CARD_H + CARD_GAP),
                ),
                vec2(CARD_W, CARD_H),
            );
            match item {
                Item::Account(idx) => self.account_card(ui, platform, *idx, rect, entrance, now),
                Item::Folder { id, name } => self.folder_card(ui, id, name, rect, entrance),
            }
        }
    }

    fn folder_card(
        &mut self,
        ui: &mut Ui,
        id: &'static str,
        name: &'static str,
        rect: Rect,
        entrance: f32,
    ) {
        let t = self.theme;
        let back = id == "..";
        let resp = ui.interact(rect, Id::new(("folder", id)), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        if resp.clicked() {
            self.folder = if back { None } else { Some(id) };
            self.armed = None;
        }
        let h = ease_out(anim(ui, resp.id, resp.hovered(), 0.18));
        let mut painter = ui.painter().clone();
        painter.set_opacity(entrance);
        let rect = scale_about_center(rect, 0.98 + 0.02 * entrance).translate(vec2(0.0, -2.0 * h));

        if h > 0.0 {
            shadow(&painter, rect, 0.18 * h);
        }
        painter.rect_filled(
            rect,
            CornerRadius::same(8),
            Rgba::TRANSPARENT.lerp(t.card_hover, h).to_color32(),
        );
        let outline = t.fg_subtle.mix(0.45, Rgba::TRANSPARENT);
        painter.rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.0, Rgba::TRANSPARENT.lerp(outline, h).to_color32()),
            StrokeKind::Outside,
        );

        let icon_box = Rect::from_min_size(
            pos2(rect.center().x - AVATAR / 2.0, rect.top() + CARD_PAD - h),
            Vec2::splat(AVATAR),
        );
        let box_bg = t.muted.mix(0.52, Rgba::TRANSPARENT).lerp(t.elevated, h);
        painter.rect_filled(icon_box, CornerRadius::same(6), box_bg.to_color32());
        let color = t.fg_muted.lerp(t.fg, h);
        let svg = if back { icons::BACK } else { icons::FOLDER };
        let size = if back { 26.0 } else { 30.0 };
        let tex = self.icon(ui.ctx(), svg, size);
        painter.image(
            tex.id(),
            Rect::from_center_size(icon_box.center(), Vec2::splat(size)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            color.to_color32(),
        );
        name_label(&painter, rect, name, t.fg);
    }

    fn account_card(
        &mut self,
        ui: &mut Ui,
        platform: Platform,
        idx: usize,
        rect: Rect,
        entrance: f32,
        now: f64,
    ) {
        let t = self.theme;
        let account = self.data.accounts(platform)[idx].clone();
        let id = Id::new(("card", account.id.clone()));
        let resp = ui
            .interact(rect, id, Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &account.name)
        });

        let is_active = self.data.active(platform) == account.id;
        let is_switching = self
            .switching
            .as_ref()
            .is_some_and(|(s, _)| *s == account.id);
        if resp.clicked() && self.switching.is_none() {
            if self.armed.as_deref() == Some(account.id.as_str()) {
                self.armed = None;
                self.switching = Some((account.id.clone(), now));
            } else {
                self.armed = Some(account.id.clone());
            }
        }
        let armed = self.armed.as_deref() == Some(account.id.as_str());

        let lift = !is_active;
        let h = ease_out(anim(ui, id.with("hover"), resp.hovered() && lift, 0.18));
        let press = anim(
            ui,
            id.with("press"),
            resp.is_pointer_button_down_on() && lift,
            0.08,
        );
        let blur = ease_out(anim(ui, id.with("blur"), armed || is_switching, 0.3));
        let confirm = ease_out(anim(ui, id.with("confirm"), armed, 0.15));

        let mut painter = ui.painter().clone();
        painter.set_opacity(entrance);
        let scale = (0.98 + 0.02 * entrance) * (1.0 - 0.015 * press);
        let rect = scale_about_center(rect, scale).translate(vec2(0.0, -2.0 * h * (1.0 - press)));

        // Surface.
        let custom = account.color.map(|c| Rgba::hex(c, 1.0));
        let rest = match custom {
            Some(c) => c.mix(0.24, t.card),
            None if is_active => t.card_hover,
            None => t.card,
        };
        let hover_bg = match custom {
            Some(c) if !is_active => c.mix(0.32, t.card_hover),
            _ => rest.lerp(t.card_hover, if is_active { 0.0 } else { 1.0 }),
        };
        if h > 0.0 {
            shadow(&painter, rect, 0.18 * h);
        }
        painter.rect_filled(
            rect,
            CornerRadius::same(8),
            rest.lerp(hover_bg, h).to_color32(),
        );

        // Active rings: a bright halo, then a dark band (two spread shadows).
        let active_t = ease_out(anim(ui, id.with("active"), is_active, 0.18));
        if active_t > 0.0 {
            let (halo, band, band_w) = match custom {
                Some(c) => (
                    t.fg.mix(0.72, Rgba::TRANSPARENT),
                    c.mix(0.5, t.bg_solid.mix(0.62, Rgba::TRANSPARENT)),
                    3.0,
                ),
                None => (
                    t.fg.mix(0.62, Rgba::TRANSPARENT),
                    t.bg_solid.mix(0.45, Rgba::TRANSPARENT),
                    2.0,
                ),
            };
            let fade = |c: Rgba| c.alpha(c.a * active_t).to_color32();
            painter.add(RectShape::stroke(
                rect.expand(2.0),
                CornerRadius::same(10),
                Stroke::new(band_w, fade(band)),
                StrokeKind::Outside,
            ));
            painter.add(RectShape::stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(2.0, fade(halo)),
                StrokeKind::Outside,
            ));
        }

        // Avatar box.
        let avatar = Rect::from_min_size(
            pos2(rect.center().x - AVATAR / 2.0, rect.top() + CARD_PAD - h),
            Vec2::splat(AVATAR),
        );
        let box_bg = t.muted.lerp(t.elevated, h);
        painter.rect_filled(avatar, CornerRadius::same(6), box_bg.to_color32());
        let zoom = 1.0 + 0.04 * h;
        let inset = (1.0 - 1.0 / zoom) / 2.0;
        let uv = Rect::from_min_max(pos2(inset, inset), pos2(1.0 - inset, 1.0 - inset));
        let dim = 1.0 - 0.5 * blur;
        let dim_color = |alpha: f32| {
            let v = (255.0 * dim).round() as u8;
            Color32::from_rgba_unmultiplied(v, v, v, (255.0 * alpha).round() as u8)
        };
        match account.avatar.and_then(|i| self.avatars[i].as_ref()) {
            Some(tex) => {
                let fade_in = ease_out(((now - tex.loaded_at) / 0.22) as f32);
                if fade_in < 1.0 {
                    ui.ctx().request_repaint();
                }
                painter.add(
                    RectShape::filled(
                        avatar,
                        CornerRadius::same(6),
                        dim_color(fade_in * (1.0 - blur)),
                    )
                    .with_texture(tex.sharp.id(), uv),
                );
                if blur > 0.0 {
                    painter.add(
                        RectShape::filled(avatar, CornerRadius::same(6), dim_color(fade_in * blur))
                            .with_texture(tex.blurred.id(), uv),
                    );
                }
            }
            None if account.avatar.is_none() => {
                let seed = format!(
                    "{}::{}::{}",
                    account.name,
                    account.id,
                    account.name.chars().rev().collect::<String>()
                );
                let grad = self.gradient(ui.ctx(), &seed);
                painter.add(
                    RectShape::filled(avatar, CornerRadius::same(6), dim_color(1.0)).with_texture(
                        grad.id(),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    ),
                );
                painter.text(
                    avatar.center(),
                    Align2::CENTER_CENTER,
                    data::initials(&account.name),
                    FontId::new(20.0, semibold()),
                    t.fg.alpha(1.0 - 0.5 * blur).to_color32(),
                );
            }
            None => {}
        }
        if is_active {
            painter.rect_stroke(
                avatar,
                CornerRadius::same(6),
                Stroke::new(2.0, t.fg.mix(0.2, Rgba::TRANSPARENT).to_color32()),
                StrokeKind::Outside,
            );
        }

        if is_switching {
            spinner(&painter, avatar.center(), now, t);
            ui.ctx().request_repaint();
        }
        if confirm > 0.0 {
            let size = 24.0 * (0.8 + 0.2 * confirm);
            let tex = self.icon(ui.ctx(), icons::PLAY, 24.0);
            painter.image(
                tex.id(),
                Rect::from_center_size(avatar.center(), Vec2::splat(size)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                t.fg.alpha(confirm).to_color32(),
            );
        }

        name_label(&painter, rect, &account.name, t.fg);

        if confirm > 0.0 {
            let inner_w = CARD_W - 2.0 * CARD_PAD - 6.0;
            let mut job = egui::text::LayoutJob::single_section(
                "Click again to switch".into(),
                egui::TextFormat {
                    font_id: FontId::new(9.0, semibold()),
                    color: t.fg.alpha(confirm).to_color32(),
                    line_height: Some(9.0 * 1.2),
                    ..Default::default()
                },
            );
            job.wrap.max_width = inner_w;
            job.halign = egui::Align::Center;
            let hint = painter.layout_job(job);
            let hint_rect = Rect::from_min_max(
                pos2(
                    rect.left() + CARD_PAD,
                    rect.bottom() - 4.0 - hint.size().y - 4.0,
                ),
                pos2(rect.right() - CARD_PAD, rect.bottom() - 4.0),
            );
            painter.rect_filled(
                hint_rect,
                CornerRadius::same(4),
                t.bg_solid
                    .mix(0.82, Rgba::TRANSPARENT)
                    .alpha(0.82 * confirm)
                    .to_color32(),
            );
            // A centred job lays out around x = 0.
            let pos = pos2(hint_rect.center().x, hint_rect.top() + 2.0);
            painter.galley(pos, hint, t.fg.to_color32());
        }
    }

    // ---------------------------------------------------------------- list

    fn list(&mut self, ui: &mut Ui, platform: Platform, items: &[Item], entrance: f32) {
        let t = self.theme;
        let width = ui.available_width() - SCROLL_GUTTER - 32.0;
        let row_h = 44.0;
        let height = 8.0 + items.len() as f32 * (row_h + 4.0);
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let mut painter = ui.painter().clone();
        painter.set_opacity(entrance);
        for (i, item) in items.iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(
                    area.left() + 16.0,
                    area.top() + 8.0 + i as f32 * (row_h + 4.0),
                ),
                vec2(width, row_h),
            );
            let (name, last, avatar, id) = match item {
                Item::Account(idx) => {
                    let a = &self.data.accounts(platform)[*idx];
                    (
                        a.name.clone(),
                        data::relative_time(a.last_login),
                        a.avatar,
                        a.id.clone(),
                    )
                }
                Item::Folder { id, name } => {
                    (name.to_string(), String::new(), None, id.to_string())
                }
            };
            let resp = ui.interact(rect, Id::new(("row", id.clone())), Sense::click());
            let h = ease_out(anim(ui, resp.id, resp.hovered(), 0.15));
            let active = self.data.active(platform) == id;
            let bg = if active {
                t.card_hover
            } else {
                t.card.lerp(t.card_hover, h)
            };
            painter.rect_filled(rect, CornerRadius::same(8), bg.to_color32());
            if let Item::Folder { id: fid, .. } = item {
                if resp.clicked() {
                    self.folder = if *fid == ".." { None } else { Some(fid) };
                }
            }
            let av = Rect::from_min_size(rect.min + vec2(8.0, 8.0), Vec2::splat(28.0));
            painter.rect_filled(av, CornerRadius::same(4), t.muted.to_color32());
            if let Item::Folder { id: fid, .. } = item {
                let svg = if *fid == ".." {
                    icons::BACK
                } else {
                    icons::FOLDER
                };
                let tex = self.icon(ui.ctx(), svg, 16.0);
                painter.image(
                    tex.id(),
                    Rect::from_center_size(av.center(), Vec2::splat(16.0)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    t.fg_muted.to_color32(),
                );
            }
            if let Some(tex) = avatar.and_then(|a| self.avatars[a].as_ref()) {
                painter.add(
                    RectShape::filled(av, CornerRadius::same(4), Color32::WHITE).with_texture(
                        tex.sharp.id(),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    ),
                );
            }
            painter.text(
                pos2(av.right() + 10.0, rect.center().y),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(13.0),
                t.fg.to_color32(),
            );
            painter.text(
                pos2(rect.right() - 12.0, rect.center().y),
                Align2::RIGHT_CENTER,
                last,
                FontId::proportional(11.0),
                t.fg_subtle.to_color32(),
            );
        }
    }
}

// --------------------------------------------------------------- helpers

fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Segoe UI from the system, like the webview's `system-ui` fallback (Inter
/// is not installed here, so the Tauri build renders Segoe UI too).
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let read = |name: &str| std::fs::read(format!(r"{dir}\Fonts\{name}")).ok();
    let base: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    if let Some(bytes) = read("segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .expect("proportional family")
            .insert(0, "segoe".into());
    }
    let mut semibold_family = Vec::new();
    if let Some(bytes) = read("seguisb.ttf") {
        fonts.font_data.insert(
            "segoe-semibold".into(),
            Arc::new(FontData::from_owned(bytes)),
        );
        semibold_family.push("segoe-semibold".to_string());
    }
    semibold_family.extend(base);
    fonts.families.insert(semibold(), semibold_family);
    ctx.set_fonts(fonts);
}

fn button(ui: &Ui, id: Id, rect: Rect, label: &str) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp
}

fn anim(ui: &Ui, id: Id, on: bool, secs: f32) -> f32 {
    ui.ctx().animate_bool_with_time(id, on, secs)
}

fn scale_about_center(rect: Rect, s: f32) -> Rect {
    Rect::from_center_size(rect.center(), rect.size() * s)
}

/// `0 12px 24px rgba(0,0,0,a)`.
fn shadow(painter: &egui::Painter, rect: Rect, alpha: f32) {
    painter.add(
        RectShape::filled(
            rect.translate(vec2(0.0, 12.0)),
            CornerRadius::same(8),
            Color32::from_black_alpha((255.0 * alpha) as u8),
        )
        .with_blur_width(24.0),
    );
}

/// 12 px at weight 500, centred, ellipsis when too wide. The webview draws
/// 500 with the variable Segoe UI; the static semibold is the closest face.
fn name_label(painter: &egui::Painter, card: Rect, name: &str, fg: Rgba) {
    let font = FontId::new(12.0, semibold());
    let max_w = card.width() - 2.0 * CARD_PAD;
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_string(), font, fg.to_color32());
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    let galley = painter.layout_job(job);
    // Baseline where the webview puts it: line box of 14.4 px under the
    // avatar, Segoe's ascent pushing the baseline ~12.2 px into it.
    let line_top = card.top() + CARD_PAD + AVATAR + 8.0;
    let pos = pos2(
        card.center().x - galley.size().x / 2.0,
        line_top + (14.4 - galley.size().y) / 2.0 + 1.0,
    );
    painter.add(TextShape::new(pos, galley, fg.to_color32()));
}

fn spinner(painter: &egui::Painter, center: Pos2, now: f64, t: &Theme) {
    let r = 9.0;
    painter.circle_stroke(center, r, Stroke::new(2.0, t.elevated.to_color32()));
    let start = (now / 0.7 * std::f64::consts::TAU) as f32;
    let points: Vec<Pos2> = (0..=16)
        .map(|i| {
            let a = start + i as f32 / 16.0 * std::f32::consts::FRAC_PI_2;
            center + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(2.0, t.fg.to_color32())));
}

/// 145 degree linear gradient, as `getAvatarGradientStyle` paints it.
fn gradient_image(a: Rgba, b: Rgba, size: usize) -> ColorImage {
    let angle = 145f32.to_radians();
    let dir = vec2(angle.sin(), -angle.cos());
    let half = (dir.x.abs() + dir.y.abs()) / 2.0;
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let p = vec2(
                x as f32 / (size - 1) as f32 - 0.5,
                y as f32 / (size - 1) as f32 - 0.5,
            );
            let t = ((p.dot(dir) / half) + 1.0) / 2.0;
            pixels.push(a.lerp(b, t.clamp(0.0, 1.0)).to_color32());
        }
    }
    ColorImage::new([size, size], pixels)
}
