//! The fallback avatar of a card without a picture: initials over a gradient
//! derived from the account. Ported from `src/lib/shared/avatarFallback.ts`.

use crate::js;
use std::collections::HashMap;

/// `bitCycleHash`: every bit of every UTF-16 unit, low bit first.
fn bit_cycle_hash(text: &str, mult: u64, modulo: u64, seed: u64) -> u64 {
    let mut acc = seed % modulo;
    for unit in text.encode_utf16() {
        for b in 0..16u64 {
            let bit = (u64::from(unit) >> b) & 1;
            acc = (acc * mult + bit + b) % modulo;
        }
    }
    acc
}

/// `normalizeSeed` without the NFKC step: trimmed, lowercase, whitespace
/// runs folded to one space.
fn normalize_seed(seed: &str) -> String {
    js::trim(seed)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `getAvatarInitials`: the first letter of the first two words, uppercase;
/// `?` for a blank name.
pub fn avatar_initials(name: &str) -> String {
    let mut parts = name.split_whitespace();
    let Some(first) = parts.next() else {
        return "?".to_string();
    };
    let mut initials: String = first.chars().take(1).collect();
    if let Some(second) = parts.next() {
        initials.extend(second.chars().take(1));
    }
    initials.to_uppercase()
}

/// `getAvatarSeed`: the shown name (display name, else username, else
/// `unknown`), the id and the name reversed.
pub fn avatar_seed(display_name: &str, username: &str, account_id: &str) -> String {
    let primary = if !display_name.is_empty() {
        display_name
    } else {
        username
    };
    let primary = js::trim(primary);
    let stable = if primary.is_empty() {
        "unknown"
    } else {
        primary
    };
    let reversed: String = stable.chars().rev().collect();
    format!("{stable}::{account_id}::{reversed}")
}

/// The two hues of the gradient, in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AvatarGradient {
    /// `hsl(hue 72% 43%)` under the gradient, `hsl(hue 80% 56%)` at its start.
    pub hue: u16,
    /// `hsl(hue_fade 66% 38%)` at its end.
    pub hue_fade: u16,
}

impl AvatarGradient {
    /// `getAvatarGradientStyle`'s CSS, for parity checks.
    pub fn css(&self) -> String {
        let AvatarGradient { hue, hue_fade } = *self;
        format!(
            "background-color:hsl({hue} 72% 43%);background-image:linear-gradient(145deg,hsl({hue} 80% 56%),hsl({hue_fade} 66% 38%))"
        )
    }
}

/// The hues of `getAvatarGradientStyle(seed)`.
pub fn avatar_gradient(seed: &str) -> AvatarGradient {
    let normalized = normalize_seed(if seed.is_empty() { "?" } else { seed });
    let base = bit_cycle_hash(&normalized, 33, 997, 17);
    let fade = bit_cycle_hash(&normalized, 29, 991, 53);
    let hue = (base * 47) % 360;
    let hue_fade = (hue + ((fade * 61) % 181) + 37) % 360;
    AvatarGradient {
        hue: hue as u16,
        hue_fade: hue_fade as u16,
    }
}

/// `gradientCache`: one hash per distinct seed, cleared past 1000 seeds.
#[derive(Debug, Clone, Default)]
pub struct GradientCache {
    entries: HashMap<String, AvatarGradient>,
}

impl GradientCache {
    pub const MAX: usize = 1000;

    pub fn get(&mut self, seed: &str) -> AvatarGradient {
        if let Some(gradient) = self.entries.get(seed) {
            return *gradient;
        }
        let gradient = avatar_gradient(seed);
        if self.entries.len() >= Self::MAX {
            self.entries.clear();
        }
        self.entries.insert(seed.to_string(), gradient);
        gradient
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
