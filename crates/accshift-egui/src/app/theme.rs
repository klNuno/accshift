//! The theme in effect: the registry's theme for the settings (or the theme
//! editor's draft), applied with the backdrop the OS can give, and the
//! motion preference. Recomputed when one of its inputs changes.

use accshift_app::lifecycle::ui_scale::{backdrop_available, liquid_backdrop_active};
use accshift_app::settings::AnimationsMode;
use accshift_app::stores::ClientStores;
use accshift_theme::{
    theme_application, window_backdrop, AppTheme, ColorScheme, MotionAttribute, ResolvedTheme,
    ThemeApplication, ThemeDocument, ThemeRegistry, WindowBackdrop,
};
use egui::Context;
use std::time::Instant;

use super::platforms::RuntimeOs;
use super::vars::Vars;
use super::AppModel;
use crate::ui::anim::{self, Motion};

/// The inputs a theme application depends on.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ThemeKey {
    theme_id: String,
    opacity: i64,
    os: RuntimeOs,
    animations: AnimationsMode,
    custom_themes: usize,
    /// A Liquid Glass wallpaper snapshot is up.
    wallpaper: bool,
}

/// The theme in effect.
pub struct ThemeState {
    pub(super) key: ThemeKey,
    pub theme: AppTheme,
    pub application: ThemeApplication,
    pub resolved: ResolvedTheme,
    pub vars: Vars,
    /// What the window shell must put behind a transparent window.
    pub backdrop: WindowBackdrop,
    pub motion: Motion,
}

fn animations_mode(mode: AnimationsMode) -> accshift_theme::AnimationsMode {
    match mode {
        AnimationsMode::System => accshift_theme::AnimationsMode::System,
        AnimationsMode::On => accshift_theme::AnimationsMode::On,
        AnimationsMode::Off => accshift_theme::AnimationsMode::Off,
    }
}

pub(super) fn theme_key(
    stores: &ClientStores,
    registry: &ThemeRegistry,
    os: RuntimeOs,
    wallpaper: bool,
) -> ThemeKey {
    let settings = stores.settings();
    ThemeKey {
        theme_id: settings.theme_id.clone(),
        opacity: settings.background_opacity,
        os,
        animations: settings.animations,
        custom_themes: registry.all_themes().len(),
        wallpaper,
    }
}

/// The theme of the settings, applied with the backdrop the OS can give,
/// and the motion preference.
pub(super) fn compute_theme(
    registry: &ThemeRegistry,
    stores: &ClientStores,
    os: RuntimeOs,
    os_reduced_motion: bool,
    wallpaper: bool,
) -> ThemeState {
    let key = theme_key(stores, registry, os, wallpaper);
    let theme = registry.theme(Some(&key.theme_id));
    theme_state(key, theme, os_reduced_motion)
}

/// The theme editor's draft, applied with the inputs of the settings' theme.
pub(super) fn preview_theme(
    registry: &ThemeRegistry,
    stores: &ClientStores,
    os: RuntimeOs,
    os_reduced_motion: bool,
    wallpaper: bool,
    draft: &ThemeDocument,
) -> ThemeState {
    let key = theme_key(stores, registry, os, wallpaper);
    let theme = registry.theme_from_document(draft);
    theme_state(key, theme, os_reduced_motion)
}

pub(super) fn theme_state(key: ThemeKey, theme: AppTheme, os_reduced_motion: bool) -> ThemeState {
    let os = key.os;
    let liquid_backdrop_active = liquid_backdrop_active(theme.liquid, os);
    let backdrop_available = backdrop_available(os, liquid_backdrop_active, key.wallpaper);
    let application = theme_application(&theme, key.opacity as f64, backdrop_available);
    let resolved = ResolvedTheme::new(&application);
    let vars = Vars {
        dark: application.color_scheme == ColorScheme::Dark,
        ..Vars::new(&resolved)
    };
    let backdrop = window_backdrop(theme.glass, &theme.id, theme.liquid);
    let motion = match accshift_theme::motion_attribute(
        animations_mode(key.animations),
        os_reduced_motion,
    ) {
        MotionAttribute::Full => Motion::Full,
        MotionAttribute::Reduced => Motion::Reduced,
    };
    ThemeState {
        key,
        theme,
        application,
        resolved,
        vars,
        backdrop,
        motion,
    }
}

impl AppModel {
    /// Settings changed under us (a store write or an external refresh):
    /// recompute what derives from them.
    pub(super) fn sync_settings(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.stores.sync();
        // The Liquid Glass wallpaper snapshot comes and goes with the theme.
        let liquid = b.theme.theme.liquid && b.runtime_os == RuntimeOs::Windows;
        self.liquid.tick(ctx, &self.jobs, liquid, Instant::now());
        let wallpaper = self.liquid.present();
        let key = theme_key(&b.stores, &b.registry, b.runtime_os, wallpaper);
        if key != b.theme.key && b.theme_preview.is_none() {
            b.theme = compute_theme(
                &b.registry,
                &b.stores,
                b.runtime_os,
                self.os_reduced_motion,
                wallpaper,
            );
        }
        b.locale = b.stores.language();
        // Harness scenes clear egui memory: keep the motion flag in it.
        anim::set_motion(ctx, b.theme.motion);
    }
}
