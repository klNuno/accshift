//! The decoded avatars of the last session, so a card shows its picture on
//! the frame it mounts, with no fade: one file of the http avatars the
//! session uploaded, by URL and device size, read whole when the workspace
//! boots. Decoding them again took 100 to 150 ms on the job pool, then the
//! late pictures faded in for 220 ms.
//!
//! Avatar URLs of Steam and Roblox name their content, so an entry never
//! goes stale; a new picture is a new URL. The file is rewritten only when
//! the session decoded an avatar it lacked, keeping the ones this session
//! used first, under [`MAX_BYTES`].

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use egui::{Color32, ColorImage};

const MAGIC: &[u8; 8] = b"ASAVPK01";
const FILE: &str = "decoded.bin";
/// 16 MB: 800 avatars at 72 px.
const MAX_BYTES: usize = 16 << 20;

/// An entry: URL, device size, pixels.
pub type Entry = (String, u32, Arc<ColorImage>);

type Loaded = HashMap<(String, u32), Arc<ColorImage>>;

/// The read [`prefetch`] started, for the [`Pack::read`] of that folder.
static PREFETCH: Mutex<Option<(PathBuf, JoinHandle<Loaded>)>> = Mutex::new(None);

/// Starts reading the pack in `dir` on a thread, from `main`: its read and
/// parse (5 to 8 ms) then overlap the event loop and window creation
/// instead of delaying the first frame.
pub fn prefetch(dir: PathBuf) {
    let at = dir.clone();
    let read = std::thread::spawn(move || load(&at));
    if let Ok(mut slot) = PREFETCH.lock() {
        *slot = Some((dir, read));
    }
}

fn load(dir: &Path) -> Loaded {
    std::fs::read(dir.join(FILE))
        .ok()
        .and_then(|bytes| parse(&bytes))
        .unwrap_or_default()
}

/// The pack of the last session, taken entry by entry as cards ask for them.
#[derive(Default)]
pub struct Pack {
    dir: Option<PathBuf>,
    loaded: HashMap<(String, u32), Arc<ColorImage>>,
    /// A decode landed that the file lacks.
    pub dirty: bool,
}

impl Pack {
    /// Reads the pack in `dir` (the avatar disk cache); empty when there is
    /// none or it does not parse.
    pub fn read(dir: Option<PathBuf>) -> Pack {
        let prefetched = PREFETCH.lock().ok().and_then(|mut slot| slot.take());
        let loaded = match (dir.as_deref(), prefetched) {
            (Some(dir), Some((at, read))) if at == dir => read.join().unwrap_or_default(),
            (Some(dir), _) => load(dir),
            (None, _) => Loaded::default(),
        };
        Pack {
            dir,
            loaded,
            dirty: false,
        }
    }

    pub fn len(&self) -> usize {
        self.loaded.len()
    }

    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    /// The pixels of `url` at `px`, once.
    pub fn take(&mut self, url: &str, px: u32) -> Option<Arc<ColorImage>> {
        if self.is_empty() {
            return None;
        }
        self.loaded.remove(&(url.to_string(), px))
    }

    /// What to write: `used` (this session's), then what is left of the
    /// last pack, under the size cap. None when nothing new was decoded.
    pub fn take_write(&mut self, used: Vec<Entry>) -> Option<(PathBuf, Vec<Entry>)> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        let dir = self.dir.clone()?;
        let mut total = 0;
        let rest = self
            .loaded
            .iter()
            .map(|((url, px), image)| (url.clone(), *px, Arc::clone(image)));
        let entries = used
            .into_iter()
            .chain(rest)
            .take_while(|(url, _, image)| {
                total += url.len() + 16 + image.pixels.len() * 4;
                total <= MAX_BYTES
            })
            .collect();
        Some((dir, entries))
    }
}

/// Writes `entries` to `dir`, through a temporary file so a reader never
/// sees half of it.
pub fn write(dir: &Path, entries: &[Entry]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut out = Vec::with_capacity(
        12 + entries
            .iter()
            .map(|(url, _, image)| url.len() + 16 + image.pixels.len() * 4)
            .sum::<usize>(),
    );
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (url, px, image) in entries {
        out.extend_from_slice(&(url.len() as u32).to_le_bytes());
        out.extend_from_slice(url.as_bytes());
        for n in [*px, image.size[0] as u32, image.size[1] as u32] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        for c in &image.pixels {
            out.extend_from_slice(&c.to_array());
        }
    }
    let tmp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&out)?;
    drop(file);
    std::fs::rename(&tmp, dir.join(FILE)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn parse(bytes: &[u8]) -> Option<Loaded> {
    let mut rest = bytes.strip_prefix(MAGIC)?;
    let u32_at = |rest: &mut &[u8]| -> Option<u32> {
        let (n, tail) = rest.split_first_chunk::<4>()?;
        *rest = tail;
        Some(u32::from_le_bytes(*n))
    };
    let count = u32_at(&mut rest)?;
    let mut map = HashMap::with_capacity(count.min(4096) as usize);
    for _ in 0..count {
        let len = u32_at(&mut rest)? as usize;
        let url = std::str::from_utf8(rest.get(..len)?).ok()?.to_string();
        rest = &rest[len..];
        let px = u32_at(&mut rest)?;
        let (w, h) = (u32_at(&mut rest)? as usize, u32_at(&mut rest)? as usize);
        let n = w.checked_mul(h)?.checked_mul(4)?;
        let pixels = rest
            .get(..n)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Color32::from_rgba_premultiplied(r, g, b, a))
            .collect();
        rest = &rest[n..];
        map.insert((url, px), Arc::new(ColorImage::new([w, h], pixels)));
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(seed: u8) -> Arc<ColorImage> {
        let pixels = (0..6)
            .map(|i| Color32::from_rgba_premultiplied(seed, i, 0, 200))
            .collect();
        Arc::new(ColorImage::new([3, 2], pixels))
    }

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("accshift-pack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_written_pack_reads_back() {
        let dir = dir("round");
        let a = image(1);
        write(&dir, &[("https://a/1.jpg".into(), 68, Arc::clone(&a))]).unwrap();
        let mut pack = Pack::read(Some(dir.clone()));
        assert_eq!(pack.len(), 1);
        assert_eq!(pack.take("https://a/1.jpg", 102), None);
        let back = pack.take("https://a/1.jpg", 68).unwrap();
        assert_eq!(back.size, [3, 2]);
        assert_eq!(back.pixels, a.pixels);
        assert_eq!(pack.take("https://a/1.jpg", 68), None, "taken once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn it_writes_only_after_a_new_decode_and_keeps_unused_entries() {
        let dir = dir("dirty");
        write(
            &dir,
            &[("u1".into(), 68, image(1)), ("u2".into(), 68, image(2))],
        )
        .unwrap();
        let mut pack = Pack::read(Some(dir.clone()));
        let used = pack.take("u1", 68).unwrap();
        assert!(pack
            .take_write(vec![("u1".into(), 68, used.clone())])
            .is_none());
        pack.dirty = true;
        let (_, entries) = pack
            .take_write(vec![("u1".into(), 68, used), ("u3".into(), 68, image(3))])
            .unwrap();
        let urls: Vec<_> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(urls, ["u1", "u3", "u2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_file_reads_as_empty() {
        let dir = dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), b"ASAVPK01\x05\0\0\0\x10").unwrap();
        assert!(Pack::read(Some(dir.clone())).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
