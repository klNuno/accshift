//! `src/lib/shared/avatarFadeIn.ts` with the avatar `<img>` rule of
//! `AccountCard.svelte`: the picture starts at `opacity: 0` and is revealed
//! once its bytes are decoded (or failed), through `transition: opacity
//! var(--motion-avatar-fade) ease-out`.
//!
//! A picture already decoded when the card mounts (`node.complete`) is
//! revealed before the first style: no fade. One decoded later fades from 0.
//! That is exactly a CSS transition whose first value is the state at mount,
//! so each mount of a card needs its own id.

use egui::{Context, Id};

use crate::app::vars::Vars;
use crate::ui::anim::{self, Timing, Transition};

/// The `<img>` opacity this frame. `revealed`: the texture is uploaded (or
/// the load failed).
pub fn avatar_opacity(ctx: &Context, id: Id, revealed: bool, vars: &Vars) -> f32 {
    anim::transition_bool(
        ctx,
        id.with("avatar-fade-in"),
        revealed,
        &Transition::new(vars.motion_avatar_fade, Timing::EASE_OUT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars::new(&accshift_theme::ResolvedTheme::new(
            &accshift_theme::theme_application(
                &accshift_theme::ThemeRegistry::new().theme(Some("dark")),
                100.0,
                true,
            ),
        ))
    }

    fn at(ctx: &Context, t: f64, f: impl Fn(&Context) -> f32) -> f32 {
        let mut out = 0.0;
        let input = egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| out = f(ui.ctx()));
        output.textures_delta.clear();
        out
    }

    #[test]
    fn cached_pictures_show_at_once_late_ones_fade() {
        let v = vars();
        let ctx = Context::default();
        let id = Id::new("card");
        assert_eq!(at(&ctx, 0.0, |c| avatar_opacity(c, id, true, &v)), 1.0);
        let late = Id::new("late");
        assert_eq!(at(&ctx, 0.0, |c| avatar_opacity(c, late, false, &v)), 0.0);
        let mid = at(&ctx, 0.1, |c| avatar_opacity(c, late, true, &v));
        assert!(mid < 0.05, "starts the fade at the decode: {mid}");
        let half = at(&ctx, 0.21, |c| avatar_opacity(c, late, true, &v));
        assert!(half > 0.5 && half < 1.0, "{half}");
        assert_eq!(at(&ctx, 0.4, |c| avatar_opacity(c, late, true, &v)), 1.0);
    }
}
