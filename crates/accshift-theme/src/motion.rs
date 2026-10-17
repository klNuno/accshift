//! Port of the motion mode of `src/lib/theme/motion.ts`: what
//! `data-motion` on `<html>` says. Listening to the OS setting stays with the
//! app; it calls [`motion_attribute`] again when that setting changes.

/// The animations setting (`AnimationsMode` in the settings types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimationsMode {
    System,
    On,
    Off,
}

impl AnimationsMode {
    pub fn from_name(name: &str) -> Option<AnimationsMode> {
        match name {
            "system" => Some(AnimationsMode::System),
            "on" => Some(AnimationsMode::On),
            "off" => Some(AnimationsMode::Off),
            _ => None,
        }
    }

    /// Whether the OS reduced-motion setting must be watched (`system` only).
    pub fn follows_os(self) -> bool {
        self == AnimationsMode::System
    }
}

/// `data-motion` on `<html>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionAttribute {
    Full,
    Reduced,
}

impl MotionAttribute {
    pub fn as_str(self) -> &'static str {
        match self {
            MotionAttribute::Full => "full",
            MotionAttribute::Reduced => "reduced",
        }
    }
}

/// `off`, or `system` while the OS asks for reduced motion, reduces.
pub fn motion_attribute(mode: AnimationsMode, os_prefers_reduced_motion: bool) -> MotionAttribute {
    let reduced = mode == AnimationsMode::Off
        || (mode == AnimationsMode::System && os_prefers_reduced_motion);
    if reduced {
        MotionAttribute::Reduced
    } else {
        MotionAttribute::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_wins_over_the_os_except_in_system_mode() {
        assert_eq!(
            motion_attribute(AnimationsMode::On, true),
            MotionAttribute::Full
        );
        assert_eq!(
            motion_attribute(AnimationsMode::Off, false),
            MotionAttribute::Reduced
        );
        assert_eq!(
            motion_attribute(AnimationsMode::System, true),
            MotionAttribute::Reduced
        );
        assert_eq!(
            motion_attribute(AnimationsMode::System, false),
            MotionAttribute::Full
        );
        assert!(AnimationsMode::System.follows_os());
        assert!(!AnimationsMode::Off.follows_os());
    }
}
