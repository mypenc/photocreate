//! Artwork timelapse: a small frame is recorded after each stroke (or any edit) and the frames
//! play back or export as an animated GIF.
//!
//! Frames live beside the artwork in `<artwork>.pcraft.timelapse/NNNNN.idx`: one byte per pixel
//! into a fixed 3-3-2 palette, at most [`MAX_SIDE`] points on the long edge. No decoder is needed
//! to play them back, and 600 frames of a tablet-size canvas stay near 100 MB at the very most
//! ([`MAX_FRAMES`] thins the oldest half when the cap is reached, so a long session keeps its
//! whole arc). Recording never blocks painting: a frame is taken only while no pointer is down.

use std::path::{Path, PathBuf};

use photocraft_codecs::web::{GifFrame, encode_gif_animated};

use crate::PhotocraftApp;

/// Long edge of a recorded frame, in pixels.
pub const MAX_SIDE: u32 = 480;
/// Frames kept per artwork before the oldest are thinned.
pub const MAX_FRAMES: usize = 600;
/// Shortest gap between two recorded frames, seconds.
const MIN_GAP: f64 = 0.8;
const MAGIC: &[u8; 4] = b"PCT1";

/// The folder holding an artwork's frames.
pub fn dir_for(artwork: &Path) -> PathBuf {
    let mut s = artwork.as_os_str().to_owned();
    s.push(".timelapse");
    PathBuf::from(s)
}

/// The fixed 3-3-2 palette every frame uses.
pub fn palette() -> Vec<[u8; 3]> {
    (0..256u32)
        .map(|i| {
            let (r, g, b) = ((i >> 5) & 7, (i >> 2) & 7, i & 3);
            [(r * 255 / 7) as u8, (g * 255 / 7) as u8, (b * 255 / 3) as u8]
        })
        .collect()
}

fn index_of(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) >> 5) << 5 | (u32::from(g) >> 5) << 2 | (u32::from(b) >> 6)) as u8
}

/// One recorded frame: indices into [`palette`].
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub indices: Vec<u8>,
}

impl Frame {
    /// The frame as an egui image (for the player and the gallery thumbnail).
    pub fn to_color_image(&self) -> egui::ColorImage {
        let pal = palette();
        let px = self.indices.iter().map(|&i| pal.get(usize::from(i)).map_or(egui::Color32::BLACK, |c| egui::Color32::from_rgb(c[0], c[1], c[2]))).collect();
        egui::ColorImage::new([self.width as usize, self.height as usize], px)
    }
}

fn encode(f: &Frame) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + f.indices.len());
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(&f.width.to_le_bytes());
    v.extend_from_slice(&f.height.to_le_bytes());
    v.extend_from_slice(&f.indices);
    v
}

/// Read one frame file. Hostile or truncated files are an `None`, never a panic.
pub fn read_frame(path: &Path) -> Option<Frame> {
    let b = std::fs::read(path).ok()?;
    if b.get(..4)? != MAGIC {
        return None;
    }
    let w = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
    let h = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let n = (w as usize).checked_mul(h as usize)?;
    if w == 0 || h == 0 || w > 4096 || h > 4096 || b.len() != 12 + n {
        return None;
    }
    Some(Frame { width: w, height: h, indices: b.get(12..)?.to_vec() })
}

/// Frame files of `dir`, oldest first.
pub fn frame_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "idx")).collect();
    v.sort();
    v
}

/// Keep every other frame of the older half so a long session keeps its whole arc.
fn thin(dir: &Path) {
    let files = frame_files(dir);
    if files.len() < MAX_FRAMES {
        return;
    }
    for (i, p) in files.iter().take(files.len() / 2).enumerate() {
        if i % 2 == 1 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Encode the recorded frames as an animated GIF, `fps` frames a second (the last frame holds
/// for a second). `None` when nothing was recorded.
pub fn export_gif(artwork: &Path, fps: u16) -> Option<Vec<u8>> {
    let files = frame_files(&dir_for(artwork));
    let frames: Vec<Frame> = files.iter().filter_map(|p| read_frame(p)).collect();
    let first = frames.first()?;
    let (w, h) = (first.width, first.height);
    let delay = (100 / fps.max(1)).max(2);
    let pal = palette();
    let n = frames.len();
    let gif: Vec<GifFrame> = frames
        .into_iter()
        .enumerate()
        .filter(|(_, f)| f.width == w && f.height == h)
        .map(|(i, f)| GifFrame { indices: f.indices, palette: pal.clone(), transparent: None, delay_cs: if i + 1 == n { 100 } else { delay } })
        .collect();
    encode_gif_animated(w, h, &gif, true).ok()
}

/// Records frames for the active document.
#[derive(Default)]
pub struct Recorder {
    /// Document identity and revision last recorded.
    last: Option<(u64, u64)>,
    last_time: f64,
    /// A change is waiting for the pointer to lift or the gap to pass.
    dirty: bool,
    next: u32,
    dir: Option<PathBuf>,
    pub enabled: bool,
}

impl Recorder {
    pub fn new() -> Self {
        Self { enabled: true, ..Self::default() }
    }

    /// Once per frame. Takes a frame after an edit when no pointer is down.
    pub fn tick(&mut self, app: &PhotocraftApp, ctx: &egui::Context) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(st) = app.session.active() else { return false };
        let Some(path) = st.path.as_deref().map(PathBuf::from) else { return false };
        let key = (st.doc.id.0, st.revision);
        let dir = dir_for(&path);
        if self.dir.as_deref() != Some(dir.as_path()) {
            // A different artwork: continue its numbering.
            self.next = frame_files(&dir).len() as u32;
            self.dir = Some(dir.clone());
            self.last = Some(key);
            self.dirty = false;
            if self.next == 0 {
                self.dirty = true; // the opening state is frame one
            }
        } else if self.last != Some(key) {
            self.last = Some(key);
            self.dirty = true;
        }
        let now = ctx.input(|i| i.time);
        if !self.dirty || now - self.last_time < MIN_GAP || ctx.input(|i| i.pointer.any_down()) {
            if self.dirty {
                ctx.request_repaint_after(std::time::Duration::from_millis(400));
            }
            return false;
        }
        let (w0, h0) = (st.doc.size.width.max(1), st.doc.size.height.max(1));
        let k = (MAX_SIDE as f32 / w0.max(h0) as f32).min(1.0);
        let (w, h) = (((w0 as f32 * k) as u32).max(1), ((h0 as f32 * k) as u32).max(1));
        let img = photocraft_compose::render_reduced(&st.doc, w, h).to_rgba8();
        let mut indices = Vec::with_capacity(w as usize * h as usize);
        for px in img.pixels.chunks_exact(4) {
            // Over white, as the artwork looks on paper.
            let a = u32::from(px[3]);
            let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
            indices.push(index_of(over(px[0]), over(px[1]), over(px[2])));
        }
        let frame = Frame { width: img.width, height: img.height, indices };
        if std::fs::create_dir_all(&dir).is_ok() {
            let file = dir.join(format!("{:05}.idx", self.next));
            if std::fs::write(&file, encode(&frame)).is_ok() {
                self.next += 1;
                thin(&dir);
            }
        }
        self.last_time = now;
        self.dirty = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_256_entries_and_round_trips_white_and_black() {
        let p = palette();
        assert_eq!(p.len(), 256);
        assert_eq!(p[usize::from(index_of(255, 255, 255))], [255, 255, 255]);
        assert_eq!(p[usize::from(index_of(0, 0, 0))], [0, 0, 0]);
    }

    #[test]
    fn frames_round_trip_and_hostile_files_are_none() {
        let dir = std::env::temp_dir().join(format!("pc-tl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let f = Frame { width: 2, height: 2, indices: vec![1, 2, 3, 4] };
        let p = dir.join("00000.idx");
        std::fs::write(&p, encode(&f)).unwrap_or_default();
        assert_eq!(read_frame(&p).map(|f| f.indices), Some(vec![1, 2, 3, 4]));
        std::fs::write(&p, b"PCT1\xff\xff\xff\xff\xff\xff\xff\xffxx").unwrap_or_default();
        assert!(read_frame(&p).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
