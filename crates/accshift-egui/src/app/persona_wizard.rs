//! `PersonaWizard.svelte`'s script: the create and edit form of a persona.
//!
//! Mounted fresh per open, as the `{#if editing}` block does: the fields
//! start from the persona being edited (or empty), and a baseline of them
//! tells a dirty form from an untouched one. Leaving a dirty form takes a
//! second press within 3 s (`confirmingCancel`).
//!
//! The custom image goes through the system file picker on a job thread
//! (`<input type="file" accept="image/*">`), then is cropped to a 256 px
//! square cover and stored as a data URL, as `handleImageFile` does with a
//! canvas.

use std::collections::BTreeMap;

use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::personas::{
    persona_assignments_for_save, AssignmentsForSave, Persona, PersonaAssignment,
};

use super::personas::PersonasPanel;

/// `cancelResetTimer`: the armed back button disarms after 3 s.
pub const DISARM_AFTER: f64 = 3.0;

/// The cover's side, px: `handleImageFile`'s canvas.
const COVER_SIZE: u32 = 256;
/// `toDataURL(type, 0.85)`.
const COVER_QUALITY: u8 = 85;

/// What a finished image pick brings back.
#[derive(Debug, PartialEq, Eq)]
pub enum ImagePick {
    /// The picker was closed without a file.
    Cancelled,
    /// The file is a cover now.
    Cover(String),
    /// The file could not be read or decoded: `personas.imageError`.
    Failed,
}

pub struct PersonaWizard {
    /// `persona`: the one being edited, `None` to create one.
    pub persona: Option<Persona>,
    pub name: String,
    pub image: Option<String>,
    /// `selection`: platform id to picked account id, `""` for none.
    pub selection: BTreeMap<String, String>,
    /// `picking`: the platform whose account is being picked.
    pub picking: Option<String>,
    /// `confirmingCancel`: when the back button armed (ctx time).
    pub armed_at: Option<f64>,
    baseline: (String, Option<String>, Vec<PersonaAssignment>),
    /// `page-entrance`'s start (ctx time), set on the first frame.
    pub opened_at: Option<f64>,
    /// `onMount`: a new persona's name field takes the focus once.
    pub focus_name: bool,
    image_job: Option<Pending<ImagePick>>,
}

impl PersonaWizard {
    pub fn new(persona: Option<Persona>, panel: &PersonasPanel) -> Self {
        let selection = panel
            .platforms
            .iter()
            .map(|p| {
                let id = persona
                    .as_ref()
                    .and_then(|persona| persona.assignments.iter().find(|a| a.platform_id == p.id))
                    .map(|a| a.account_id.clone())
                    .unwrap_or_default();
                (p.id.clone(), id)
            })
            .collect();
        let baseline = (
            persona.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
            persona.as_ref().and_then(|p| p.image.clone()),
            persona
                .as_ref()
                .map(|p| p.assignments.clone())
                .unwrap_or_default(),
        );
        Self {
            focus_name: persona.is_none(),
            name: baseline.0.clone(),
            image: baseline.1.clone(),
            persona,
            selection,
            picking: None,
            armed_at: None,
            baseline,
            opened_at: None,
            image_job: None,
        }
    }

    fn selected(&self, platform_id: &str) -> &str {
        self.selection.get(platform_id).map_or("", String::as_str)
    }

    /// `isValidAssignment`: a stored id is dropped only after a read that
    /// succeeded without it.
    pub fn is_valid_assignment(&self, panel: &PersonasPanel, platform_id: &str) -> bool {
        let id = self.selected(platform_id);
        if id.is_empty() {
            return false;
        }
        if panel.accounts_loading || panel.account_load_failed.iter().any(|p| p == platform_id) {
            return true;
        }
        panel
            .accounts_by_platform
            .get(platform_id)
            .is_some_and(|accounts| accounts.iter().any(|a| a.id == id))
    }

    /// `assignedAccount`.
    pub fn assigned_account<'a>(
        &self,
        panel: &'a PersonasPanel,
        platform_id: &str,
    ) -> Option<&'a accshift_app::platforms::PlatformAccount> {
        let id = self.selected(platform_id);
        if id.is_empty() {
            return None;
        }
        panel
            .accounts_by_platform
            .get(platform_id)?
            .iter()
            .find(|a| a.id == id)
    }

    /// `assignments`: what a save writes.
    pub fn assignments(&self, panel: &PersonasPanel) -> Vec<PersonaAssignment> {
        let offered: Vec<String> = panel.platforms.iter().map(|p| p.id.clone()).collect();
        let accounts: BTreeMap<String, Vec<String>> = panel
            .accounts_by_platform
            .iter()
            .map(|(platform, accounts)| {
                (
                    platform.clone(),
                    accounts.iter().map(|a| a.id.clone()).collect(),
                )
            })
            .collect();
        let original = self
            .persona
            .as_ref()
            .map_or(&[][..], |p| p.assignments.as_slice());
        persona_assignments_for_save(&AssignmentsForSave {
            original,
            offered_platform_ids: &offered,
            selection: &self.selection,
            accounts_by_platform: &accounts,
            loading: panel.accounts_loading,
            failed_platform_ids: &panel.account_load_failed,
        })
    }

    /// `dirty`.
    pub fn dirty(&self, panel: &PersonasPanel) -> bool {
        (&self.name, &self.image, &self.assignments(panel))
            != (&self.baseline.0, &self.baseline.1, &self.baseline.2)
    }

    /// `canSave`.
    pub fn can_save(&self, panel: &PersonasPanel) -> bool {
        !self.name.trim().is_empty()
            && !self.assignments(panel).is_empty()
            && !panel.accounts_loading
    }

    /// `confirmingCancel`, with the 3 s timer applied at `now`.
    pub fn armed(&self, now: f64) -> bool {
        self.armed_at.is_some_and(|at| now - at < DISARM_AFTER)
    }

    /// `requestCancel`: true when the wizard closes; otherwise it arms.
    pub fn request_cancel(&mut self, panel: &PersonasPanel, now: f64) -> bool {
        if !self.dirty(panel) || self.armed(now) {
            return true;
        }
        self.armed_at = Some(now);
        false
    }

    /// `handleEscape`: closes the picker, else steps out like the back
    /// button. True when the wizard closes.
    pub fn handle_escape(&mut self, panel: &PersonasPanel, now: f64) -> bool {
        if self.picking.take().is_some() {
            return false;
        }
        self.request_cancel(panel, now)
    }

    /// `pickAccount`.
    pub fn pick_account(&mut self, platform_id: &str, account_id: &str) {
        self.selection
            .insert(platform_id.to_string(), account_id.to_string());
        self.picking = None;
    }

    /// `unassign`.
    pub fn unassign(&mut self, platform_id: &str) {
        self.selection
            .insert(platform_id.to_string(), String::new());
        self.picking = None;
    }

    /// `fileInputRef.click()`: opens the system picker, one at a time.
    pub fn choose_image(&mut self, jobs: &Jobs) {
        if self.image_job.is_some() {
            return;
        }
        self.image_job = Some(jobs.run(|_| pick_cover()));
    }

    /// A pick still open.
    pub fn picking_image(&self) -> bool {
        self.image_job.is_some()
    }

    /// Lands a finished pick: true when it failed (`personas.imageError`).
    pub fn poll_image(&mut self) -> bool {
        let Some(job) = self.image_job.as_mut() else {
            return false;
        };
        match job.poll() {
            Poll::Waiting => false,
            Poll::Ready(pick) => {
                self.image_job = None;
                match pick {
                    ImagePick::Cover(url) => {
                        self.image = Some(url);
                        false
                    }
                    ImagePick::Cancelled => false,
                    ImagePick::Failed => true,
                }
            }
            Poll::Gone => {
                self.image_job = None;
                false
            }
        }
    }
}

/// The file picker of `accept="image/*"`: Chromium lists image files first
/// and every file second. Blocks until the picker closes.
fn pick_cover() -> ImagePick {
    let picked = rfd::FileDialog::new()
        .add_filter(
            "Image Files",
            &[
                "bmp", "jpg", "jpeg", "jfif", "pjpeg", "pjp", "png", "webp", "gif", "ico", "avif",
                "svg", "tif", "tiff",
            ],
        )
        .add_filter("All Files", &["*"])
        .pick_file();
    let Some(path) = picked else {
        return ImagePick::Cancelled;
    };
    match std::fs::read(&path)
        .ok()
        .and_then(|bytes| cover_from_bytes(&bytes))
    {
        Some(url) => ImagePick::Cover(url),
        None => ImagePick::Failed,
    }
}

/// `handleImageFile` past the decode: the image scaled to cover a 256 px
/// square, centred, as a data URL. Chromium's canvas encodes WebP at 0.85;
/// its JPEG fallback at the same quality is what this build writes.
pub fn cover_from_bytes(bytes: &[u8]) -> Option<String> {
    use base64::Engine as _;
    use image::{imageops, GenericImageView, ImageReader};

    let decoded = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let (w, h) = decoded.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let side = COVER_SIZE as f64;
    let scale = (side / w as f64).max(side / h as f64);
    let sw = ((w as f64 * scale).round() as u32).max(COVER_SIZE);
    let sh = ((h as f64 * scale).round() as u32).max(COVER_SIZE);
    let scaled = imageops::resize(&decoded.to_rgba8(), sw, sh, imageops::FilterType::Triangle);
    let (x, y) = ((sw - COVER_SIZE) / 2, (sh - COVER_SIZE) / 2);
    let cropped = imageops::crop_imm(&scaled, x, y, COVER_SIZE, COVER_SIZE).to_image();
    // The canvas starts transparent; JPEG has no alpha, so a see-through
    // pixel lands on black, as Chromium's encoder flattens it.
    let rgb: image::RgbImage = image::ImageBuffer::from_fn(COVER_SIZE, COVER_SIZE, |x, y| {
        let [r, g, b, a] = cropped.get_pixel(x, y).0;
        let k = a as u32;
        image::Rgb([
            (r as u32 * k / 255) as u8,
            (g as u32 * k / 255) as u8,
            (b as u32 * k / 255) as u8,
        ])
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, COVER_QUALITY)
        .encode_image(&rgb)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_cover_is_a_256_px_centred_jpeg() {
        use base64::Engine as _;
        let url = cover_from_bytes(&png(800, 400)).expect("cover");
        let data = url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("jpeg url");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (256, 256));
        // The 2:1 image is cropped to its middle: red left, blue right.
        let left = img.get_pixel(40, 128).0;
        let right = img.get_pixel(215, 128).0;
        assert!(left[0] > 200 && left[2] < 60, "{left:?}");
        assert!(right[2] > 200 && right[0] < 60, "{right:?}");
    }

    #[test]
    fn garbage_is_no_cover() {
        assert_eq!(cover_from_bytes(b"not an image"), None);
    }
}
