//! SVG rasterization for avatars: an SVG avatar is drawn at the device size
//! it is shown at.

use egui::ColorImage;

pub fn raster(svg: &[u8], px: u32) -> Result<ColorImage, String> {
    egui_extras::image::load_svg_bytes_with_size(
        svg,
        egui::SizeHint::Size {
            width: px,
            height: px,
            maintain_aspect_ratio: true,
        },
        &resvg::usvg::Options::default(),
    )
}
