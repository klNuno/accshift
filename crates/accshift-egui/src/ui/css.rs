//! Just enough CSS value parsing to build a [`BoxStyle`] from the
//! declarations of a component: colours, lengths, `box-shadow`,
//! `linear-gradient()`, `radial-gradient()`, `border`, `border-radius`,
//! `background` and `opacity`. Lengths are CSS px (egui points). `var()` and
//! `color-mix()` are resolved upstream (`accshift-theme`), not here.

use egui::{vec2, Vec2};

use super::paint::{
    Border, BoxShadow, BoxStyle, ColorStop, Corners, Gradient, Length, LinearDirection,
    LinearGradient, RadialGradient, RadialShape, RadialSize, Sides,
};
use crate::theme::Rgba;

/// Split on `sep` outside parentheses.
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Whitespace-separated tokens outside parentheses.
pub fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                start.get_or_insert(i);
            }
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

fn number(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok()
}

/// `12px`, `0`, `12.5px` in CSS px.
pub fn px(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("px") {
        return number(v);
    }
    let v = number(s)?;
    (v == 0.0).then_some(0.0)
}

pub fn length(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix('%') {
        return number(v).map(Length::Percent);
    }
    px(s).map(Length::Px)
}

/// Degrees from `deg`, `turn`, `rad` or `grad`.
fn angle(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("deg") {
        return number(v);
    }
    if let Some(v) = s.strip_suffix("grad") {
        return number(v).map(|g| g * 0.9);
    }
    if let Some(v) = s.strip_suffix("rad") {
        return number(v).map(f32::to_degrees);
    }
    if let Some(v) = s.strip_suffix("turn") {
        return number(v).map(|t| t * 360.0);
    }
    None
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()` in either
/// syntax, `transparent`, `black`, `white`.
pub fn color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    match s {
        "transparent" => return Some(Rgba::TRANSPARENT),
        "black" => return Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        "white" => return Some(Rgba::new(255.0, 255.0, 255.0, 1.0)),
        _ => {}
    }
    if let Some(hex) = s.strip_prefix('#') {
        let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|d| d * 17);
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        let (r, g, b, a) = match hex.len() {
            3 => (digit(0)?, digit(1)?, digit(2)?, 255),
            4 => (digit(0)?, digit(1)?, digit(2)?, digit(3)?),
            6 => (pair(0)?, pair(2)?, pair(4)?, 255),
            8 => (pair(0)?, pair(2)?, pair(4)?, pair(6)?),
            _ => return None,
        };
        return Some(Rgba::new(
            f32::from(r),
            f32::from(g),
            f32::from(b),
            f32::from(a) / 255.0,
        ));
    }
    let inner = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let (channels, alpha) = if inner.contains(',') {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        match parts.len() {
            3 => (parts, None),
            4 => (parts[..3].to_vec(), Some(parts[3])),
            _ => return None,
        }
    } else {
        let (c, a) = match inner.split_once('/') {
            Some((c, a)) => (c, Some(a.trim())),
            None => (inner, None),
        };
        (c.split_whitespace().collect(), a)
    };
    if channels.len() != 3 {
        return None;
    }
    let channel = |c: &str| -> Option<f32> {
        match c.strip_suffix('%') {
            Some(p) => number(p).map(|p| p * 2.55),
            None => number(c),
        }
    };
    let a = match alpha {
        Some(a) => match a.strip_suffix('%') {
            Some(p) => number(p)? / 100.0,
            None => number(a)?,
        },
        None => 1.0,
    };
    Some(Rgba::new(
        channel(channels[0])?.clamp(0.0, 255.0),
        channel(channels[1])?.clamp(0.0, 255.0),
        channel(channels[2])?.clamp(0.0, 255.0),
        a.clamp(0.0, 1.0),
    ))
}

/// A `box-shadow` list (`none` gives an empty list).
pub fn shadows(s: &str) -> Option<Vec<BoxShadow>> {
    if s.trim() == "none" {
        return Some(Vec::new());
    }
    split_top(s, ',').into_iter().map(shadow).collect()
}

fn shadow(s: &str) -> Option<BoxShadow> {
    let mut inset = false;
    let mut lengths = Vec::new();
    let mut col = None;
    for t in tokens(s) {
        if t == "inset" {
            inset = true;
        } else if let Some(v) = px(t) {
            lengths.push(v);
        } else {
            col = Some(color(t)?);
        }
    }
    if lengths.len() < 2 || lengths.len() > 4 {
        return None;
    }
    Some(BoxShadow {
        offset: vec2(lengths[0], lengths[1]),
        blur: lengths.get(2).copied().unwrap_or(0.0).max(0.0),
        spread: lengths.get(3).copied().unwrap_or(0.0),
        // currentcolor is not tracked: CSS would use `color`.
        color: col.unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        inset,
    })
}

fn stops(args: &[&str]) -> Option<Vec<ColorStop>> {
    let mut out = Vec::new();
    for arg in args {
        let t = tokens(arg);
        let (first, rest) = t.split_first()?;
        let c = color(first)?;
        if rest.is_empty() {
            out.push(ColorStop {
                color: c,
                position: None,
            });
        }
        for p in rest {
            out.push(ColorStop {
                color: c,
                position: Some(length(p)?),
            });
        }
    }
    (out.len() >= 2).then_some(out)
}

/// `linear-gradient()` or `radial-gradient()`.
pub fn gradient(s: &str) -> Option<Gradient> {
    let s = s.trim();
    let (name, inner) = s.strip_suffix(')')?.split_once('(')?;
    let args: Vec<&str> = split_top(inner, ',').into_iter().map(str::trim).collect();
    match name.trim() {
        "linear-gradient" => {
            let (direction, rest) = match args.first() {
                Some(a) if angle(a).is_some() => (
                    LinearDirection::Angle(angle(a).unwrap_or(180.0)),
                    &args[1..],
                ),
                Some(a) if a.starts_with("to ") => {
                    let mut x = 0i8;
                    let mut y = 0i8;
                    for w in a.split_whitespace().skip(1) {
                        match w {
                            "left" => x = -1,
                            "right" => x = 1,
                            "top" => y = -1,
                            "bottom" => y = 1,
                            _ => return None,
                        }
                    }
                    (LinearDirection::Side(x, y), &args[1..])
                }
                _ => (LinearDirection::Angle(180.0), &args[..]),
            };
            Some(Gradient::Linear(LinearGradient {
                direction,
                stops: stops(rest)?,
            }))
        }
        "radial-gradient" => {
            let head_is_stop = args
                .first()
                .and_then(|a| tokens(a).first().map(|t| color(t).is_some()))
                .unwrap_or(false);
            let mut g = RadialGradient {
                shape: RadialShape::Ellipse,
                size: RadialSize::FarthestCorner,
                center: (Length::Percent(50.0), Length::Percent(50.0)),
                stops: Vec::new(),
            };
            let rest = if head_is_stop {
                &args[..]
            } else {
                radial_head(args.first()?, &mut g)?;
                &args[1..]
            };
            g.stops = stops(rest)?;
            Some(Gradient::Radial(g))
        }
        _ => None,
    }
}

fn radial_head(head: &str, g: &mut RadialGradient) -> Option<()> {
    let t = tokens(head);
    let at = t.iter().position(|w| *w == "at").unwrap_or(t.len());
    let mut sizes = Vec::new();
    let mut shape = None;
    for w in &t[..at] {
        match *w {
            "circle" => shape = Some(RadialShape::Circle),
            "ellipse" => shape = Some(RadialShape::Ellipse),
            "closest-side" => g.size = RadialSize::ClosestSide,
            "farthest-side" => g.size = RadialSize::FarthestSide,
            "closest-corner" => g.size = RadialSize::ClosestCorner,
            "farthest-corner" => g.size = RadialSize::FarthestCorner,
            other => sizes.push(length(other)?),
        }
    }
    match sizes.as_slice() {
        [] => {}
        [r] => {
            g.size = RadialSize::Explicit(*r, *r);
            shape.get_or_insert(RadialShape::Circle);
        }
        [rx, ry] => g.size = RadialSize::Explicit(*rx, *ry),
        _ => return None,
    }
    g.shape = shape.unwrap_or(RadialShape::Ellipse);
    if at < t.len() {
        let pos = &t[at + 1..];
        let kw = |w: &str| match w {
            "left" | "top" => Some(Length::Percent(0.0)),
            "center" => Some(Length::Percent(50.0)),
            "right" | "bottom" => Some(Length::Percent(100.0)),
            other => length(other),
        };
        g.center = match pos {
            [one] => match *one {
                "top" | "bottom" => (Length::Percent(50.0), kw(one)?),
                _ => (kw(one)?, Length::Percent(50.0)),
            },
            [a, b] => {
                // `top left` is allowed in either order.
                if matches!(*a, "top" | "bottom") || matches!(*b, "left" | "right") {
                    (kw(b)?, kw(a)?)
                } else {
                    (kw(a)?, kw(b)?)
                }
            }
            _ => return None,
        };
    }
    Some(())
}

/// 1 to 4 values, CSS side order (top, right, bottom, left).
fn four<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    match *values {
        [a] => Some([a, a, a, a]),
        [a, b] => Some([a, b, a, b]),
        [a, b, c] => Some([a, b, c, b]),
        [a, b, c, d] => Some([a, b, c, d]),
        _ => None,
    }
}

/// `border-radius` without the elliptical `/` form.
pub fn radius(s: &str) -> Option<Corners> {
    let v: Option<Vec<f32>> = s.split_whitespace().map(px).collect();
    let [tl, tr, br, bl] = four(&v?)?;
    Some(Corners::new(tl, tr, br, bl))
}

/// `1px solid <color>` (only `solid` and `none` styles).
fn border_side(s: &str) -> Option<(f32, Rgba)> {
    let mut width = 3.0; // `medium`
    let mut col = Rgba::new(0.0, 0.0, 0.0, 1.0);
    let mut visible = false;
    for t in tokens(s) {
        match t {
            "solid" => visible = true,
            "none" | "hidden" => return Some((0.0, Rgba::TRANSPARENT)),
            "thin" => width = 1.0,
            "medium" => width = 3.0,
            "thick" => width = 5.0,
            other => match px(other) {
                Some(w) => width = w,
                None => col = color(other)?,
            },
        }
    }
    Some(if visible { (width, col) } else { (0.0, col) })
}

impl BoxStyle {
    /// Build a style from declarations (`prop: value; ...`). Unknown
    /// properties are ignored, invalid values return `None`.
    pub fn from_css(decls: &str) -> Option<BoxStyle> {
        let mut style = BoxStyle::default();
        for decl in split_top(decls, ';') {
            let Some((prop, value)) = decl.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_end_matches("!important").trim();
            match prop.trim() {
                "background" | "background-image" => {
                    style.images.clear();
                    for layer in split_top(value, ',') {
                        let layer = layer.trim();
                        if layer == "none" {
                            continue;
                        }
                        // A layer is an image, and the last one may also
                        // carry the colour: `linear-gradient(...) #3a3d4a`.
                        for t in tokens(layer) {
                            if let Some(g) = gradient(t) {
                                style.images.push(g);
                            } else {
                                style.background = color(t)?;
                            }
                        }
                    }
                }
                "background-color" => style.background = color(value)?,
                "border-radius" => style.radius = radius(value)?,
                "border" => {
                    let (w, c) = border_side(value)?;
                    style.border = Border::all(w, c);
                }
                "border-top" | "border-right" | "border-bottom" | "border-left" => {
                    let (w, c) = border_side(value)?;
                    let side = &prop.trim()["border-".len()..];
                    let (width, col) = match side {
                        "top" => (&mut style.border.width.top, &mut style.border.color.top),
                        "right" => (&mut style.border.width.right, &mut style.border.color.right),
                        "bottom" => (
                            &mut style.border.width.bottom,
                            &mut style.border.color.bottom,
                        ),
                        _ => (&mut style.border.width.left, &mut style.border.color.left),
                    };
                    *width = w;
                    *col = c;
                }
                "border-width" => {
                    let v: Option<Vec<f32>> = value.split_whitespace().map(px).collect();
                    let [t, r, b, l] = four(&v?)?;
                    style.border.width = Sides::new(t, r, b, l);
                }
                "border-color" => {
                    let v: Option<Vec<Rgba>> = tokens(value).into_iter().map(color).collect();
                    let [t, r, b, l] = four(&v?)?;
                    style.border.color = Sides::new(t, r, b, l);
                }
                "box-shadow" => style.shadows = shadows(value)?,
                "opacity" => style.opacity = number(value)?.clamp(0.0, 1.0),
                _ => {}
            }
        }
        Some(style)
    }
}

/// `left`, `top`, `width`, `height` in px from the same declarations.
pub fn position(decls: &str) -> Option<(Vec2, Vec2)> {
    let mut pos = Vec2::ZERO;
    let mut size = Vec2::ZERO;
    for decl in split_top(decls, ';') {
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        let v = || px(value);
        match prop.trim() {
            "left" => pos.x = v()?,
            "top" => pos.y = v()?,
            "width" => size.x = v()?,
            "height" => size.y = v()?,
            _ => {}
        }
    }
    Some((pos, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(color("#fff").unwrap(), Rgba::new(255.0, 255.0, 255.0, 1.0));
        let c = color("rgba(234, 179, 8, 0.45)").unwrap();
        assert_eq!((c.r, c.g, c.b), (234.0, 179.0, 8.0));
        assert!((c.a - 0.45).abs() < 1e-6);
        let c = color("rgb(0 0 0 / 0.32)").unwrap();
        assert!((c.a - 0.32).abs() < 1e-6);
        assert!((color("#00000080").unwrap().a - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn shadow_lists() {
        let s =
            shadows("inset 0 12px 24px -18px rgba(255, 255, 255, 0.4), 0 0 0 1px #fff").unwrap();
        assert_eq!(s.len(), 2);
        assert!(s[0].inset);
        assert_eq!(s[0].offset, vec2(0.0, 12.0));
        assert_eq!((s[0].blur, s[0].spread), (24.0, -18.0));
        assert!(!s[1].inset && s[1].spread == 1.0);
    }

    #[test]
    fn gradients() {
        let Some(Gradient::Linear(g)) =
            gradient("linear-gradient(135deg, #6d28d9 0%, #1d4ed8 55%, #0891b2)")
        else {
            panic!("linear")
        };
        assert_eq!(g.direction, LinearDirection::Angle(135.0));
        assert_eq!(g.stops.len(), 3);
        let Some(Gradient::Radial(r)) =
            gradient("radial-gradient(48% 62% at 16% 20%, rgba(109,40,217,.42), transparent 72%)")
        else {
            panic!("radial")
        };
        assert_eq!(
            r.size,
            RadialSize::Explicit(Length::Percent(48.0), Length::Percent(62.0))
        );
        assert_eq!(r.center, (Length::Percent(16.0), Length::Percent(20.0)));
        assert_eq!(r.stops[1].position, Some(Length::Percent(72.0)));
    }

    #[test]
    fn declarations() {
        let s = BoxStyle::from_css(
            "background: #2a2a34; border: 1px solid rgba(255,255,255,.2); border-radius: 12.5px; box-shadow: 0 8px 24px rgb(0 0 0 / 0.32)",
        )
        .unwrap();
        assert_eq!(s.radius, Corners::all(12.5));
        assert_eq!(s.border.width, Sides::all(1.0));
        assert_eq!(s.shadows.len(), 1);
    }
}
