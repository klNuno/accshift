use egui::Vec2;

use crate::theme::Rgba;

/// Top, right, bottom, left, like CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    pub const fn new(top: T, right: T, bottom: T, left: T) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn all(v: T) -> Self {
        Self::new(v, v, v, v)
    }

    pub fn map<U: Copy>(self, f: impl Fn(T) -> U) -> Sides<U> {
        Sides::new(f(self.top), f(self.right), f(self.bottom), f(self.left))
    }
}

/// Per-corner radii in CSS px, horizontal and vertical (elliptical corners).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners {
    pub top_left: Vec2,
    pub top_right: Vec2,
    pub bottom_right: Vec2,
    pub bottom_left: Vec2,
}

impl Corners {
    pub const ZERO: Corners = Corners::all(0.0);

    pub const fn all(r: f32) -> Self {
        let v = Vec2 { x: r, y: r };
        Self {
            top_left: v,
            top_right: v,
            bottom_right: v,
            bottom_left: v,
        }
    }

    /// Circular corners, CSS order.
    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            top_left: Vec2 {
                x: top_left,
                y: top_left,
            },
            top_right: Vec2 {
                x: top_right,
                y: top_right,
            },
            bottom_right: Vec2 {
                x: bottom_right,
                y: bottom_right,
            },
            bottom_left: Vec2 {
                x: bottom_left,
                y: bottom_left,
            },
        }
    }

    pub(super) fn to_array(self) -> [Vec2; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }
}

/// `border-*-width` (CSS px) and `border-*-color`, style `solid`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: Sides<f32>,
    pub color: Sides<Rgba>,
}

impl Border {
    pub const NONE: Border = Border {
        width: Sides::all(0.0),
        color: Sides::all(Rgba::TRANSPARENT),
    };

    pub const fn all(width: f32, color: Rgba) -> Self {
        Self {
            width: Sides::all(width),
            color: Sides::all(color),
        }
    }
}

impl Default for Border {
    fn default() -> Self {
        Self::NONE
    }
}

/// One `box-shadow` entry, lengths in CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
    pub inset: bool,
}

/// A `<length-percentage>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
}

impl Length {
    /// Device pixels, percentages of `basis` (device pixels).
    pub(super) fn device(self, basis: f32, ppp: f32) -> f32 {
        match self {
            Length::Px(v) => v * ppp,
            Length::Percent(p) => p / 100.0 * basis,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub color: Rgba,
    pub position: Option<Length>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearDirection {
    /// CSS angle in degrees: 0 points up, 90 right.
    Angle(f32),
    /// `to <side-or-corner>`: x and y in -1, 0, 1 (y down).
    Side(i8, i8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    pub direction: LinearDirection,
    pub stops: Vec<ColorStop>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialShape {
    Circle,
    Ellipse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialSize {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    Explicit(Length, Length),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadialGradient {
    pub shape: RadialShape,
    pub size: RadialSize,
    pub center: (Length, Length),
    pub stops: Vec<ColorStop>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Gradient {
    Linear(LinearGradient),
    Radial(RadialGradient),
}

/// The painted properties of one CSS box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle {
    pub background: Rgba,
    /// `background-image` layers, first on top.
    pub images: Vec<Gradient>,
    pub radius: Corners,
    pub border: Border,
    /// First on top, like CSS.
    pub shadows: Vec<BoxShadow>,
    pub opacity: f32,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            background: Rgba::TRANSPARENT,
            images: Vec::new(),
            radius: Corners::ZERO,
            border: Border::NONE,
            shadows: Vec::new(),
            opacity: 1.0,
        }
    }
}

/// A 2D `transform` on a box: `translate(translate) scale(scale)` around
/// the box centre (`transform-origin: center`), in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate: Vec2,
    pub scale: f32,
}

impl Transform {
    pub const NONE: Transform = Transform {
        translate: Vec2::ZERO,
        scale: 1.0,
    };

    pub const fn translate(translate: Vec2) -> Self {
        Self {
            translate,
            scale: 1.0,
        }
    }

    pub const fn scale(scale: f32) -> Self {
        Self {
            translate: Vec2::ZERO,
            scale,
        }
    }
}
