//! The avatar texture cache of the workspace.
//!
//! M2's [`AvatarImages`] turns an avatar URL into an [`AvatarImage`] (a
//! local file at once, an http download through the job pool). Here each
//! image is decoded off the UI thread through [`Jobs`] at the device size
//! the card paints it, then uploaded on the UI thread, keyed by the image's
//! stable key and that size. The card fades the picture in on its own
//! (`avatar_fade_in`).
//!
//! Decoders: SVG (resvg, the mock's avatars) and PNG. JPEG, what Steam's CDN
//! serves, needs a decoder this crate does not link yet: such a picture
//! fails and the card shows its initials.

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
    Ready(TextureHandle),
    Failed,
}

/// URL to image (M2) and image to texture (here).
pub struct AvatarTextures {
    images: AvatarImages,
    http: Option<Arc<HttpAvatarClient>>,
    downloads: Vec<(String, Pending<Result<AvatarImage, String>>)>,
    textures: HashMap<(String, u32), Entry>,
}

impl AvatarTextures {
    pub fn new(policy: UrlPolicy) -> Self {
        Self {
            images: AvatarImages::new(policy),
            http: None,
            downloads: Vec::new(),
            textures: HashMap::new(),
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
                    *entry = Entry::Ready(ctx.load_texture(
                        format!("avatar-{key}-{px}"),
                        image,
                        TextureOptions::LINEAR,
                    ));
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
            Entry::Ready(texture) => AvatarTexture::Ready {
                texture: texture.id(),
                size: texture.size(),
            },
            Entry::Failed => AvatarTexture::Failed,
        }
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

/// Decodes an avatar for a `px` device pixel box. SVGs rasterize at that
/// size (covering it, like `object-fit: cover`); bitmaps keep theirs.
pub fn decode(image: &AvatarImage, px: u32) -> Result<ColorImage, String> {
    let bytes: Arc<[u8]> = match image {
        AvatarImage::File { path, .. } => std::fs::read(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .into(),
        AvatarImage::Bytes { bytes, .. } => Arc::clone(bytes),
    };
    if bytes.starts_with(b"\x89PNG") {
        return decode_png(&bytes);
    }
    if is_svg(&bytes) {
        return crate::avatars::raster(&bytes, px.max(1));
    }
    Err("unsupported avatar format".into())
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg"))
}

fn decode_png(bytes: &[u8]) -> Result<ColorImage, String> {
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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../src/lib/mock/avatars/1.svg");
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
        let image = AvatarImage::Bytes {
            key: "k".into(),
            bytes: out.into(),
        };
        let decoded = decode(&image, 68).unwrap();
        assert_eq!(decoded.size, [2, 1]);
        assert_eq!(decoded.pixels[0], Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn jpeg_is_reported_unsupported() {
        let image = AvatarImage::Bytes {
            key: "j".into(),
            bytes: vec![0xff, 0xd8, 0xff, 0xe0].into(),
        };
        assert!(decode(&image, 68).is_err());
    }
}
