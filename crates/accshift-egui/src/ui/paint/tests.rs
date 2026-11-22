use egui::{pos2, vec2, Rect, Vec2};

use super::geometry::{BoxGeometry, RRect};
use super::gradient::resolve_stops;
use super::shadow::render_shadow_mask;
use super::*;
use crate::theme::Rgba;

#[test]
fn overlapping_radii_scale_together() {
    // border-radius: 999px on a 90x60 box becomes a pill of radius 30.
    let rr = RRect::new(0.0, 0.0, 90.0, 60.0, [Vec2::splat(999.0); 4]);
    for c in rr.r {
        assert!((c.x - 30.0).abs() < 1e-3 && (c.y - 30.0).abs() < 1e-3);
    }
}

#[test]
fn coverage_is_a_one_pixel_ramp() {
    let rr = RRect::new(10.0, 10.0, 50.0, 50.0, [Vec2::splat(8.0); 4]);
    assert_eq!(rr.coverage(vec2(30.0, 30.0)), 1.0);
    assert!((rr.coverage(vec2(10.0, 30.0)) - 0.5).abs() < 1e-6);
    assert_eq!(rr.coverage(vec2(9.4, 30.0)), 0.0);
    // On the corner arc, half covered.
    let c = vec2(18.0, 18.0) - vec2(8.0, 8.0) * std::f32::consts::FRAC_1_SQRT_2;
    assert!((rr.coverage(c) - 0.5).abs() < 1e-3);
}

#[test]
fn spread_follows_the_css_radius_adjustment() {
    let rr = RRect::new(0.0, 0.0, 100.0, 100.0, [Vec2::splat(4.0); 4]);
    let s = rr.spread(8.0);
    // r = 4, s = 8: 4 + 8 * (1 + (0.5 - 1)^3) = 11.
    assert!((s.r[0].x - 11.0).abs() < 1e-4);
    assert_eq!(s.x0, -8.0);
    let square = RRect::new(0.0, 0.0, 10.0, 10.0, [Vec2::ZERO; 4]).spread(2.0);
    assert_eq!(square.r[0], Vec2::ZERO);
}

#[test]
fn boxes_sit_on_the_device_grid() {
    let border = Border::all(1.0, Rgba::TRANSPARENT);
    let r = Rect::from_min_size(pos2(200.5, 14.5), vec2(30.0, 16.0));
    let geo = BoxGeometry::new(r, Transform::NONE, Corners::all(4.0), &border, 1.5);
    let bb = geo.border_box;
    assert_eq!([bb.x0, bb.y0, bb.x1, bb.y1], [301.0, 22.0, 346.0, 46.0]);
    assert_eq!(geo.widths.top, 1.0);
    let geo = BoxGeometry::new(
        r,
        Transform::translate(vec2(0.5, 0.0)),
        Corners::all(4.0),
        &border,
        1.5,
    );
    assert_eq!(geo.border_box.x0, 301.75);
}

#[test]
fn stops_fix_up_like_css() {
    let red = Rgba::new(255.0, 0.0, 0.0, 1.0);
    let stops = [
        ColorStop {
            color: red,
            position: None,
        },
        ColorStop {
            color: red,
            position: None,
        },
        ColorStop {
            color: red,
            position: Some(Length::Percent(40.0)),
        },
        ColorStop {
            color: red,
            position: Some(Length::Percent(20.0)),
        },
        ColorStop {
            color: red,
            position: None,
        },
    ];
    let p: Vec<f32> = resolve_stops(&stops, 100.0, 1.0)
        .iter()
        .map(|s| s.0)
        .collect();
    assert_eq!(p, vec![0.0, 0.2, 0.4, 0.4, 1.0]);
}

#[test]
fn gaussian_mask_is_half_on_the_edge() {
    // A wide box, blur 8 (sigma 4): the outer shadow at the edge is half
    // the blurred coverage outside the box and zero under it.
    let shape = RRect::new(20.0, 20.0, 80.0, 80.0, [Vec2::ZERO; 4]);
    let clip = RRect::new(200.0, 200.0, 201.0, 201.0, [Vec2::ZERO; 4]);
    let img = render_shadow_mask([100, 100], 4.0, &clip, &shape, true);
    let a = |x: usize, y: usize| img.pixels[y * 100 + x].a();
    let edge = (i32::from(a(19, 50)) + i32::from(a(20, 50))) / 2;
    assert!((edge - 128).abs() <= 4, "edge {edge}");
    assert!(a(50, 50) >= 254);
    assert!(a(5, 50) < 3);
}
