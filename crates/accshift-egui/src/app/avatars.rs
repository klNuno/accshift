//! The avatar texture cache of the workspace.
//!
//! M2's [`AvatarImages`] turns an avatar URL into an [`AvatarImage`] (a
//! local file at once, an http download through the job pool). Here each
//! image is decoded off the UI thread through [`Jobs`] at the device size
//! the card paints it, then uploaded on the UI thread, keyed by the image's
//! stable key and that size. The card fades the picture in on its own
//! (`avatar_fade_in`).
//!
//! Decoders: SVG (resvg, the mock's avatars), PNG, and JPEG (what Steam's
//! CDN serves). A bitmap is cropped to its centred square (`object-fit:
//! cover` in a square box) and scaled to the box as WebView2 scales it
//! ([`crate::ui::resample`]).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use accshift_app::accounts::avatars::{
    fetch_avatar_image, AvatarFetch, AvatarImage, AvatarImages, HttpAvatarClient,
};
use accshift_app::accounts::profile_cache::UrlPolicy;
use accshift_app::jobs::{Jobs, Pending, Poll};
use egui::{Color32, ColorImage, Context, TextureHandle, TextureId, TextureOptions};

/// What a card can draw for an avatar URL this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvatarTexture {
    Ready {
        texture: TextureId,
        size: [usize; 2],
    },
    /// Downloading or decoding.
    Pending,
    /// Not drawable: the card shows its initials.
    Failed,
}

enum Entry {
    Decoding(Pending<Result<ColorImage, String>>),
    /// The texture, and its pixels for the blurred copy.
    Ready(TextureHandle, Arc<ColorImage>),
    Failed,
}

/// URL to image (M2) and image to texture (here).
pub struct AvatarTextures {
    images: AvatarImages,
    http: Option<Arc<HttpAvatarClient>>,
    downloads: Vec<(String, Pending<Result<AvatarImage, String>>)>,
    textures: HashMap<(String, u32), Entry>,
    /// `blur(6px) brightness(.5)` copies, by image, box size and sigma.
    blurred: HashMap<(String, u32, u32), TextureHandle>,
}

impl AvatarTextures {
    pub fn new(policy: UrlPolicy) -> Self {
        Self {
            images: AvatarImages::new(policy),
            http: None,
            downloads: Vec::new(),
            textures: HashMap::new(),
            blurred: HashMap::new(),
        }
    }

    /// Lands finished downloads and decodes. Call once per frame.
    pub fn poll(&mut self, ctx: &Context) {
        let images = &mut self.images;
        self.downloads
            .retain_mut(|(url, pending)| match pending.poll() {
                Poll::Waiting => true,
                Poll::Ready(result) => {
                    images.finish(url, result);
                    false
                }
                Poll::Gone => {
                    images.finish(url, Err("download job died".into()));
                    false
                }
            });
        for ((key, px), entry) in self.textures.iter_mut() {
            let Entry::Decoding(pending) = entry else {
                continue;
            };
            match pending.poll() {
                Poll::Waiting => {}
                Poll::Ready(Ok(image)) => {
                    let image = Arc::new(image);
                    *entry = Entry::Ready(
                        ctx.load_texture(
                            format!("avatar-{key}-{px}"),
                            (*image).clone(),
                            TextureOptions::LINEAR,
                        ),
                        image,
                    );
                }
                Poll::Ready(Err(_)) | Poll::Gone => *entry = Entry::Failed,
            }
        }
    }

    /// The texture of `url` at `px` device pixels, starting its download or
    /// decode if needed.
    pub fn get(&mut self, jobs: &Jobs, url: &str, px: u32) -> AvatarTexture {
        let image = match self.images.request(url) {
            AvatarFetch::Ready(image) => image,
            AvatarFetch::Waiting => return AvatarTexture::Pending,
            AvatarFetch::Failed => return AvatarTexture::Failed,
            AvatarFetch::Start(url) => {
                let http = match &self.http {
                    Some(http) => Arc::clone(http),
                    None => match HttpAvatarClient::new() {
                        Ok(client) => {
                            let client = Arc::new(client);
                            self.http = Some(Arc::clone(&client));
                            client
                        }
                        Err(error) => {
                            self.images.finish(&url, Err(error));
                            return AvatarTexture::Failed;
                        }
                    },
                };
                let target = url.clone();
                let pending = jobs.run(move |_| fetch_avatar_image(&target, http.as_ref()));
                self.downloads.push((url, pending));
                return AvatarTexture::Pending;
            }
        };
        let key = (image.key().to_string(), px);
        let entry = self
            .textures
            .entry(key)
            .or_insert_with(|| Entry::Decoding(jobs.run(move |_| decode(&image, px))));
        match entry {
            Entry::Decoding(_) => AvatarTexture::Pending,
            Entry::Ready(texture, _) => AvatarTexture::Ready {
                texture: texture.id(),
                size: texture.size(),
            },
            Entry::Failed => AvatarTexture::Failed,
        }
    }

    /// The picture of `url` decoded for a `px` box, covered into that box
    /// and filtered with `blur(sigma) brightness(.5)` (`sigma` in device
    /// px), once it is decoded. Made on the UI thread on first use: the box
    /// is small and the card wants it the frame it arms.
    pub fn blurred(&mut self, ctx: &Context, url: &str, px: u32, sigma: f32) -> Option<TextureId> {
        let AvatarFetch::Ready(image) = self.images.request(url) else {
            return None;
        };
        let key = image.key().to_string();
        let Some(Entry::Ready(_, pixels)) = self.textures.get(&(key.clone(), px)) else {
            return None;
        };
        let slot = (key, px, (sigma * 64.0).round() as u32);
        if let Some(texture) = self.blurred.get(&slot) {
            return Some(texture.id());
        }
        let filtered = blur_cover(pixels, px.max(1) as usize, sigma, 0.5);
        let texture = ctx.load_texture(
            format!("avatar-blur-{}-{px}", slot.0),
            filtered,
            TextureOptions::LINEAR,
        );
        let id = texture.id();
        self.blurred.insert(slot, texture);
        Some(id)
    }

    /// Downloads or decodes still running.
    pub fn busy(&self) -> bool {
        !self.downloads.is_empty()
            || self
                .textures
                .values()
                .any(|entry| matches!(entry, Entry::Decoding(_)))
    }
}

/// `object-fit: cover` of `image` into a `px` square (bilinear), then CSS
/// `blur(sigma)` (a gaussian with that standard deviation, transparent
/// outside the box, in premultiplied sRGB like Skia) and
/// `brightness(factor)`.
pub fn blur_cover(image: &ColorImage, px: usize, sigma: f32, factor: f32) -> ColorImage {
    let [w, h] = image.size;
    let side = w.min(h).max(1) as f32;
    let (x0, y0) = ((w as f32 - side) / 2.0, (h as f32 - side) / 2.0);
    let sample = |x: f32, y: f32| -> [f32; 4] {
        let x = (x - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (y - 0.5).clamp(0.0, (h - 1) as f32);
        let (xi, yi) = (x.floor() as usize, y.floor() as usize);
        let (xj, yj) = ((xi + 1).min(w - 1), (yi + 1).min(h - 1));
        let (fx, fy) = (x - xi as f32, y - yi as f32);
        let px = |x: usize, y: usize| image.pixels[y * w + x].to_array().map(f32::from);
        let (a, b, c, d) = (px(xi, yi), px(xj, yi), px(xi, yj), px(xj, yj));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    };
    let scale = side / px as f32;
    let mut buf: Vec<[f32; 4]> = (0..px * px)
        .map(|i| {
            let (x, y) = ((i % px) as f32 + 0.5, (i / px) as f32 + 0.5);
            sample(x0 + x * scale, y0 + y * scale)
        })
        .collect();
    let radius = (3.0 * sigma).ceil().max(0.0) as isize;
    let kernel: Vec<f32> = {
        let raw: Vec<f32> = (-radius..=radius)
            .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma).max(1e-6)).exp())
            .collect();
        let sum: f32 = raw.iter().sum();
        raw.into_iter().map(|k| k / sum).collect()
    };
    let n = px as isize;
    for horizontal in [true, false] {
        let src = buf.clone();
        for y in 0..n {
            for x in 0..n {
                let mut acc = [0.0f32; 4];
                for (k, weight) in kernel.iter().enumerate() {
                    let d = k as isize - radius;
                    let (sx, sy) = if horizontal { (x + d, y) } else { (x, y + d) };
                    if sx < 0 || sy < 0 || sx >= n || sy >= n {
                        continue;
                    }
                    let p = src[(sy * n + sx) as usize];
                    for c in 0..4 {
                        acc[c] += p[c] * weight;
                    }
                }
                buf[(y * n + x) as usize] = acc;
            }
        }
    }
    let pixels = buf
        .into_iter()
        .map(|[r, g, b, a]| {
            let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            Color32::from_rgba_premultiplied(q(r * factor), q(g * factor), q(b * factor), q(a))
        })
        .collect();
    ColorImage::new([px, px], pixels)
}

/// Decodes an avatar for a `px` device pixel box, covering it like
/// `object-fit: cover`: SVGs rasterize at that size, bitmaps are cropped to
/// their centred square then scaled to it.
pub fn decode(image: &AvatarImage, px: u32) -> Result<ColorImage, String> {
    let bytes: Arc<[u8]> = match image {
        AvatarImage::File { path, .. } => std::fs::read(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .into(),
        AvatarImage::Bytes { bytes, .. } => Arc::clone(bytes),
    };
    if bytes.starts_with(b"\x89PNG") {
        return Ok(cover(&decode_png(&bytes)?, px));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok(cover(&decode_jpeg(&bytes)?, px));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Ok(cover(&decode_webp(&bytes)?, px));
    }
    if is_svg(&bytes) {
        return crate::avatars::raster(&bytes, px.max(1));
    }
    Err("unsupported avatar format".into())
}

/// The centred square of `image`, scaled to `px` device pixels.
fn cover(image: &ColorImage, px: u32) -> ColorImage {
    let [w, h] = image.size;
    let side = w.min(h);
    let square = if w == h {
        image.clone()
    } else {
        let (x0, y0) = ((w - side) / 2, (h - side) / 2);
        let pixels = (0..side)
            .flat_map(|y| image.pixels[(y0 + y) * w + x0..][..side].iter().copied())
            .collect();
        ColorImage::new([side, side], pixels)
    };
    crate::ui::resample::resample(&square, px.max(1) as usize)
}

/// A baseline or progressive JPEG, as sRGB (Blink applies no profile to a
/// JPEG that carries none, which is what Steam serves).
fn decode_jpeg(bytes: &[u8]) -> Result<ColorImage, String> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?
        .into_rgb8();
    let (w, h) = (decoded.width() as usize, decoded.height() as usize);
    let pixels = decoded
        .as_raw()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[r, g, b]| Color32::from_rgb(r, g, b))
        .collect();
    Ok(ColorImage::new([w, h], pixels))
}

/// A still WebP, lossy or lossless: the covers Chromium's canvas encoded.
fn decode_webp(bytes: &[u8]) -> Result<ColorImage, String> {
    let mut decoder =
        image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    let mut buf = vec![0; decoder.output_buffer_size().ok_or("webp too large")?];
    decoder.read_image(&mut buf).map_err(|e| e.to_string())?;
    let pixels = if decoder.has_alpha() {
        buf.as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
            .collect()
    } else {
        buf.as_chunks::<3>()
            .0
            .iter()
            .map(|&[r, g, b]| Color32::from_rgb(r, g, b))
            .collect()
    };
    Ok(ColorImage::new([w as usize, h as usize], pixels))
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg"))
}

pub(crate) fn decode_png(bytes: &[u8]) -> Result<ColorImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let data = &buf[..info.buffer_size()];
    let (w, h) = (info.width as usize, info.height as usize);
    let pixels: Vec<Color32> = match info.color_type {
        png::ColorType::Rgba => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
            .collect(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[r, g, b]| Color32::from_rgb(r, g, b))
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[g, a]| Color32::from_rgba_unmultiplied(g, g, g, a))
            .collect(),
        png::ColorType::Grayscale => data.iter().map(|&g| Color32::from_gray(g)).collect(),
        png::ColorType::Indexed => return Err("indexed png not expanded".into()),
    };
    if pixels.len() != w * h {
        return Err("png size mismatch".into());
    }
    Ok(ColorImage::new([w, h], pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_svg_rasterizes_at_the_device_size() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/avatars/1.svg");
        let image = AvatarImage::File {
            key: "1".into(),
            path: root,
        };
        let decoded = decode(&image, 102).unwrap();
        assert_eq!(decoded.size, [102, 102]);
    }

    #[test]
    fn png_round_trips() {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 128])
                .unwrap();
        }
        assert_eq!(
            decode_png(&out).unwrap().pixels[0],
            Color32::from_rgb(255, 0, 0)
        );
        let image = AvatarImage::Bytes {
            key: "k".into(),
            bytes: out.into(),
        };
        // A 2 x 1 picture covers the box with its left pixel.
        let decoded = decode(&image, 4).unwrap();
        assert_eq!(decoded.size, [4, 4]);
        assert_eq!(decoded.pixels[5], Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn jpeg_decodes_and_covers_the_box() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/avatars/photo.jpg");
        let image = AvatarImage::File {
            key: "photo".into(),
            path: root,
        };
        let decoded = decode(&image, 102).unwrap();
        assert_eq!(decoded.size, [102, 102]);
        // The face's yellow at the centre, through the JPEG's chroma.
        let c = decoded.pixels[50 * 102 + 51];
        assert!(c.r() > 220 && c.g() > 180 && c.b() < 130, "{c:?}");
        let broken = AvatarImage::Bytes {
            key: "j".into(),
            bytes: vec![0xff, 0xd8, 0xff, 0xe0].into(),
        };
        assert!(decode(&broken, 68).is_err());
    }
}
