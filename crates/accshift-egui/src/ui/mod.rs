//! The foundation every ported component builds on: CSS boxes painted like
//! Chromium ([`paint`]) on its device-pixel grid ([`snap`]), the CSS values
//! that describe them ([`css`]) and CSS animation timing ([`anim`]).

pub mod anim;
pub mod components;
pub mod css;
pub mod paint;
pub mod snap;
pub mod text;
