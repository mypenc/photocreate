//! Gallery: the start screen of the phone and tablet layout. It lists the artworks saved on the
//! device, newest first, each with a thumbnail and its name. Tap one to open it; its menu renames
//! or deletes it. There are no folders: every artwork is a `.pcraft` file in one place (the
//! app's documents folder).
//!
//! The thumbnail of an artwork is a small hidden file beside it (`.<file>.thumb`), written every
//! time the artwork is saved ([`save_active`]). The file operations are plain functions
//! ([`list`], [`rename`], [`delete`] ...) so they are tested without a window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{Align, Align2, Color32, FontId, Layout, RichText, Sense, TextureHandle, Vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::icons;
use crate::theme::Tokens;

/// The artwork file extension.
pub const EXT: &str = "pcraft";

/// Canvas sizes offered for a new artwork: label, width, height (pixels).
pub const PRESETS: [(&str, u32, u32); 5] = [
    ("Square 2048", 2048, 2048),
    ("Portrait 1536 x 2048", 1536, 2048),
    ("Landscape 2048 x 1536", 2048, 1536),
    ("HD 1920 x 1080", 1920, 1080),
    ("A4 300 dpi, 2480 x 3508", 2480, 3508),
];

/// Long edge of a thumbnail, in pixels.
const THUMB_SIDE: u32 = 320;
const MAGIC: &[u8; 4] = b"PCG1";
/// Seconds a message stays on screen.
const TOAST_SECONDS: f64 = 2.5;

/// One artwork in the gallery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub modified: Option<SystemTime>,
}

fn is_art(p: &Path) -> bool {
    p.extension().is_some_and(|x| x.eq_ignore_ascii_case(EXT))
}

/// The artworks in `dir`, newest first. Hidden files and other file types are left out.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let file = path.file_name()?.to_string_lossy().into_owned();
            if file.starts_with('.') || !path.is_file() || !is_art(&path) {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let modified = e.metadata().ok().and_then(|m| m.modified().ok());
            Some(Entry { path, name, modified })
        })
        .collect();
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

/// A name a person typed, made safe as a file name: no separators, no leading dots, trimmed.
/// `None` when nothing usable is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') && !c.is_control()).collect();
    let s = s.trim().trim_start_matches('.').trim().to_string();
    let s: String = s.chars().take(80).collect();
    (!s.is_empty()).then_some(s)
}

/// `name` as an artwork path inside `dir` that does not exist yet (adds " 2", " 3" ...).
pub fn unique(dir: &Path, name: &str) -> PathBuf {
    let file = |n: &str| dir.join(format!("{n}.{EXT}"));
    let mut p = file(name);
    let mut i = 2;
    while p.exists() && i < 10_000 {
        p = file(&format!("{name} {i}"));
        i += 1;
    }
    p
}

/// Where the thumbnail of `art` lives.
pub fn thumb_path(art: &Path) -> PathBuf {
    let file = art.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    art.with_file_name(format!(".{file}.thumb"))
}

/// Rename an artwork (its thumbnail follows). Returns the new path.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let name = clean_name(new_name).ok_or("Type a name")?;
    let dir = path.parent().ok_or("No folder")?;
    let target = dir.join(format!("{name}.{EXT}"));
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        return Err(format!("\"{name}\" already exists"));
    }
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    let thumb = thumb_path(path);
    if thumb.is_file() {
        let _ = std::fs::rename(thumb, thumb_path(&target));
    }
    Ok(target)
}

/// Delete an artwork and its thumbnail.
pub fn delete(path: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(thumb_path(path));
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

/// Write the thumbnail of `doc` for the artwork `art`: the composite over white, at most
/// [`THUMB_SIDE`] pixels on the long edge, as `PCG1`, width, height (little endian), RGB bytes.
pub fn write_thumb(art: &Path, doc: &photocraft_doc::Document) {
    let (w0, h0) = (doc.size.width.max(1), doc.size.height.max(1));
    let k = (THUMB_SIDE as f32 / w0.max(h0) as f32).min(1.0);
    let (w, h) = (((w0 as f32 * k) as u32).max(1), ((h0 as f32 * k) as u32).max(1));
    let img = photocraft_compose::render_reduced(doc, w, h).to_rgba8();
    let mut out = Vec::with_capacity(12 + img.pixels.len() / 4 * 3);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&img.width.to_le_bytes());
    out.extend_from_slice(&img.height.to_le_bytes());
    for px in img.pixels.chunks_exact(4) {
        let a = u32::from(px[3]);
        let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        out.extend_from_slice(&[over(px[0]), over(px[1]), over(px[2])]);
    }
    let _ = std::fs::write(thumb_path(art), out);
}

/// Read the thumbnail of `art`. A missing, truncated or hostile file is `None`.
pub fn read_thumb(art: &Path) -> Option<egui::ColorImage> {
    let b = std::fs::read(thumb_path(art)).ok()?;
    if b.get(..4)? != MAGIC {
        return None;
    }
    let w = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
    let h = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let n = (w as usize).checked_mul(h as usize)?.checked_mul(3)?;
    if w == 0 || h == 0 || w > 2048 || h > 2048 || b.len() != 12 + n {
        return None;
    }
    Some(egui::ColorImage::from_rgb([w as usize, h as usize], b.get(12..)?))
}

/// What the person is doing to one artwork.
#[derive(Clone, Debug)]
pub enum Edit {
    Rename(PathBuf, String),
    Delete(PathBuf),
}

/// Gallery state.
#[derive(Default)]
pub struct Gallery {
    /// The screen is showing.
    pub open: bool,
    /// The folder holding all artworks. Set by the host (Android: the app's documents folder).
    pub root: Option<PathBuf>,
    pub edit: Option<Edit>,
    /// The artwork whose menu is open.
    pub sheet: Option<PathBuf>,
    pub new_art: bool,
    pub new_name: String,
    /// The last problem, shown under the title.
    pub status: Option<String>,
    /// A short message over the canvas; the time it first showed.
    toast: Option<(String, Option<f64>)>,
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
}

impl Gallery {
    /// The folder of the artworks, falling back to `~/PhotoCraft` (created on demand).
    pub fn root(&mut self) -> PathBuf {
        if let Some(r) = &self.root {
            return r.clone();
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let r = home.join("PhotoCraft");
        let _ = std::fs::create_dir_all(&r);
        self.root = Some(r.clone());
        r
    }

    fn thumb(&mut self, ctx: &egui::Context, art: &Path) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(art) {
            return t.clone();
        }
        let tex = read_thumb(art).map(|img| ctx.load_texture(format!("thumb-{}", art.display()), img, egui::TextureOptions::LINEAR));
        self.thumbs.insert(art.to_path_buf(), tex.clone());
        tex
    }

    /// Forget cached thumbnails (after a save, rename or delete).
    pub fn refresh(&mut self) {
        self.thumbs.clear();
    }

    /// Show `message` over the canvas for a moment.
    pub fn notify(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), None));
    }
}

/// Save the active document as an artwork in the gallery folder, without any dialog, and write
/// its thumbnail. A document that already is an artwork is saved over itself; anything else (a
/// new or imported image) becomes a new artwork named after it. With `as_new` it is always saved
/// under a new name. Returns the path written.
pub fn save_active(app: &mut PhotocraftApp, as_new: bool) -> Result<PathBuf, String> {
    let root = app.gallery.root();
    let _ = std::fs::create_dir_all(&root);
    let path = {
        let st = app.session.active().ok_or("Nothing to save")?;
        let own = st.path.as_deref().map(PathBuf::from).filter(|p| is_art(p));
        match own {
            Some(p) if !as_new => p,
            Some(p) => {
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                unique(&root, &format!("{stem} copy"))
            }
            None => unique(&root, &clean_name(&st.doc.name).unwrap_or_else(|| "Untitled".into())),
        }
    };
    save_to(app, &path)
}

fn save_to(app: &mut PhotocraftApp, path: &Path) -> Result<PathBuf, String> {
    let result = app.run("file.save", json!({ "path": path.to_string_lossy() }));
    match result {
        Ok(_) => {
            if let Some(st) = app.session.active() {
                write_thumb(path, &st.doc);
            }
            app.gallery.refresh();
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            app.gallery.notify(format!("Saved {name}"));
            Ok(path.to_path_buf())
        }
        Err(e) => {
            app.gallery.notify(format!("Save failed: {e}"));
            Err(e)
        }
    }
}

/// Save the active artwork when it has unsaved changes. Called before the gallery opens, so
/// leaving the canvas never loses work.
pub fn save_if_changed(app: &mut PhotocraftApp) {
    let changed = app.session.active().is_some_and(|st| st.revision != st.saved_revision);
    if changed {
        let _ = save_active(app, false);
    }
}

/// Draw a short message over the canvas. Called once per frame.
pub fn toast(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let Some((text, started)) = app.gallery.toast.as_mut() else { return };
    let start = *started.get_or_insert(now);
    if now - start > TOAST_SECONDS {
        app.gallery.toast = None;
        return;
    }
    let t = Tokens::get(ctx);
    let text = text.clone();
    egui::Area::new(egui::Id::new("gallery-toast")).anchor(Align2::CENTER_BOTTOM, [0.0, -88.0]).interactable(false).order(egui::Order::Tooltip).show(ctx, |ui| {
        egui::Frame::NONE.fill(t.card).stroke(egui::Stroke::new(1.0, t.card_border)).corner_radius(t.radius).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
            ui.label(RichText::new(text).color(t.text));
        });
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(120));
}

/// `s` laid out on one line no wider than `max_w`, cut with ".." when it is longer.
fn one_line(p: &egui::Painter, s: &str, font: FontId, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let full = s.chars().count();
    let mut n = full;
    loop {
        let text = if n == full { s.to_string() } else { format!("{}..", s.chars().take(n).collect::<String>()) };
        let g = p.layout_no_wrap(text, font.clone(), color);
        if g.size().x <= max_w || n <= 1 {
            return g;
        }
        n -= 1;
    }
}

fn button(ui: &mut egui::Ui, label: &str, width: f32) -> bool {
    ui.add(egui::Button::new(label).min_size(Vec2::new(width, 44.0))).clicked()
}

/// Draw the gallery. Returns `true` while it is showing.
pub fn show(app: &mut PhotocraftApp, ui: &mut egui::Ui) -> bool {
    if !app.gallery.open {
        return false;
    }
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let root = app.gallery.root();
    let mut open: Option<PathBuf> = None;
    let mut sheet: Option<PathBuf> = None;
    let has_doc = !app.session.documents().is_empty();

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.heading("My artworks");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if has_doc && icons::button(ui, "x", 44.0, false, "Back to canvas").clicked() {
                app.gallery.open = false;
            }
            let new = egui::Button::new(RichText::new("New artwork").strong().color(t.primary_text)).fill(t.primary_bg).min_size(Vec2::new(0.0, 40.0));
            if ui.add(new).clicked() {
                app.gallery.new_art = true;
                app.gallery.new_name = "Untitled".into();
            }
        });
    });
    if let Some(s) = app.gallery.status.clone() {
        ui.label(RichText::new(s).small().color(t.danger));
    }
    ui.separator();

    let entries = list(&root);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if entries.is_empty() {
            ui.add_space(24.0);
            ui.label(RichText::new("Nothing saved yet. Tap New artwork to start drawing.").color(t.text_dim));
            return;
        }
        let gap = 12.0;
        let cols = (((ui.available_width() + gap) / (150.0 + gap)).floor() as usize).max(2);
        let card_w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).clamp(110.0, 240.0);
        let label_h = 40.0;
        egui::Grid::new("gallery-grid").spacing([gap, gap]).show(ui, |ui| {
            for (i, e) in entries.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(card_w, card_w + label_h), Sense::click());
                let p = ui.painter().clone();
                let hovered = resp.hovered();
                p.rect_filled(rect, t.radius, if hovered { t.hover } else { t.card });
                p.rect_stroke(rect, t.radius, egui::Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
                // The thumbnail, fitted into the square above the name.
                let square = egui::Rect::from_min_size(rect.min, Vec2::splat(card_w)).shrink(8.0);
                if let Some(tex) = app.gallery.thumb(&ctx, &e.path) {
                    let s = tex.size_vec2();
                    let k = (square.width() / s.x.max(1.0)).min(square.height() / s.y.max(1.0));
                    let r = egui::Rect::from_center_size(square.center(), s * k);
                    p.rect_filled(r, 2.0, Color32::WHITE);
                    p.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                } else {
                    p.rect_filled(square, t.radius_sm, t.field);
                    icons::paint(ui, square, "image", 40.0, t.text_faint);
                }
                let galley = one_line(&p, &e.name, FontId::proportional(14.0), t.text, card_w - 16.0);
                p.galley(egui::pos2(rect.left() + 8.0, rect.bottom() - label_h + 10.0), galley, t.text);
                if resp.clicked() {
                    open = Some(e.path.clone());
                }
                // The menu button: a big target in the card's corner.
                let dots = egui::Rect::from_min_size(egui::pos2(rect.right() - 44.0, rect.top()), Vec2::splat(44.0));
                p.circle_filled(dots.center(), 14.0, Color32::from_black_alpha(150));
                icons::paint(ui, dots, "ellipsis", 20.0, Color32::WHITE);
                if ui.interact(dots, resp.id.with("menu"), Sense::click()).clicked() {
                    sheet = Some(e.path.clone());
                    open = None;
                }
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
    });

    if let Some(p) = open {
        open_artwork(app, &p);
    }
    if let Some(p) = sheet {
        app.gallery.sheet = Some(p);
    }
    sheet_window(app, &ctx);
    dialogs(app, &ctx, &root);
    true
}

fn open_artwork(app: &mut PhotocraftApp, path: &Path) {
    match app.run("file.open", json!({ "path": path.to_string_lossy() })) {
        Ok(_) => {
            app.gallery.open = false;
            app.gallery.status = None;
        }
        Err(e) => app.gallery.status = Some(e),
    }
}

/// The menu of one artwork: open, rename, delete.
fn sheet_window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(path) = app.gallery.sheet.clone() else { return };
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut chosen: Option<&str> = None;
    egui::Window::new(name.clone()).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        for (key, label) in [("open", "Open"), ("rename", "Rename"), ("delete", "Delete"), ("cancel", "Cancel")] {
            if button(ui, label, 260.0) {
                chosen = Some(key);
            }
        }
    });
    if let Some(k) = chosen {
        app.gallery.sheet = None;
        match k {
            "open" => open_artwork(app, &path),
            "rename" => app.gallery.edit = Some(Edit::Rename(path, name)),
            "delete" => app.gallery.edit = Some(Edit::Delete(path)),
            _ => {}
        }
    }
}

/// An artwork that is open in the session follows a rename, and lets go of a deleted file.
fn retarget(app: &mut PhotocraftApp, from: &Path, to: Option<&Path>) {
    if let Some(st) = app.session.active_mut()
        && st.path.as_deref().map(Path::new) == Some(from)
    {
        st.path = to.map(|p| p.to_string_lossy().into_owned());
    }
}

fn dialogs(app: &mut PhotocraftApp, ctx: &egui::Context, root: &Path) {
    // New artwork: a name and a size.
    if app.gallery.new_art {
        let mut choose: Option<(u32, u32)> = None;
        let mut cancel = false;
        egui::Window::new("New artwork").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut app.gallery.new_name);
            ui.label("Canvas");
            for (label, w, h) in PRESETS {
                if button(ui, label, 240.0) {
                    choose = Some((w, h));
                }
            }
            if button(ui, "Cancel", 240.0) {
                cancel = true;
            }
        });
        if let Some((w, h)) = choose {
            let name = clean_name(&app.gallery.new_name).unwrap_or_else(|| "Untitled".into());
            let path = unique(root, &name);
            let made = app.run("file.new", json!({ "width": w, "height": h })).and_then(|_| save_to(app, &path).map(|_| ()));
            match made {
                Ok(()) => {
                    app.gallery.open = false;
                    app.gallery.status = None;
                }
                Err(e) => app.gallery.status = Some(e),
            }
            app.gallery.new_art = false;
        } else if cancel {
            app.gallery.new_art = false;
        }
    }
    // Rename / delete.
    if let Some(edit) = app.gallery.edit.clone() {
        let mut next = Some(edit.clone());
        let mut result: Option<Result<(), String>> = None;
        match edit {
            Edit::Rename(path, mut name) => {
                egui::Window::new("Rename").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.text_edit_singleline(&mut name);
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new("Rename").min_size(Vec2::new(110.0, 40.0))).clicked() {
                            result = Some(rename(&path, &name).map(|to| retarget(app, &path, Some(&to))));
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(110.0, 40.0))).clicked() {
                            result = Some(Ok(()));
                        }
                    });
                });
                if result.is_none() {
                    next = Some(Edit::Rename(path, name));
                }
            }
            Edit::Delete(path) => {
                egui::Window::new("Delete").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Delete \"{}\"? This can't be undone.", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new("Delete").min_size(Vec2::new(110.0, 40.0))).clicked() {
                            result = Some(delete(&path).map(|()| retarget(app, &path, None)));
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(110.0, 40.0))).clicked() {
                            result = Some(Ok(()));
                        }
                    });
                });
            }
        }
        match result {
            Some(r) => {
                app.gallery.status = r.err();
                app.gallery.edit = None;
                app.gallery.refresh();
            }
            None => app.gallery.edit = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pc-gallery-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  ../a/b:c  ").as_deref(), Some("abc"));
        assert_eq!(clean_name("...").as_deref(), None);
        assert_eq!(clean_name("My art").as_deref(), Some("My art"));
    }

    #[test]
    fn unique_adds_a_number() {
        let d = tmp("uq");
        let _ = std::fs::write(d.join("Moon.pcraft"), b"x");
        assert!(unique(&d, "Moon").ends_with("Moon 2.pcraft"));
        assert!(unique(&d, "Sun").ends_with("Sun.pcraft"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rename_and_delete_carry_the_thumbnail() {
        let d = tmp("mv");
        let art = d.join("a.pcraft");
        let _ = std::fs::write(&art, b"x");
        let _ = std::fs::write(thumb_path(&art), b"y");
        let renamed = rename(&art, "Moon").unwrap_or_default();
        assert!(renamed.ends_with("Moon.pcraft") && thumb_path(&renamed).exists() && !thumb_path(&art).exists());
        assert!(delete(&renamed).is_ok() && !renamed.exists() && !thumb_path(&renamed).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn list_shows_artworks_only() {
        let d = tmp("ls");
        let _ = std::fs::write(d.join("b.pcraft"), b"x");
        let _ = std::fs::write(d.join(".b.pcraft.thumb"), b"x");
        let _ = std::fs::create_dir_all(d.join("Folder"));
        let _ = std::fs::write(d.join("notes.txt"), b"x");
        let names: Vec<_> = list(&d).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["b"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn thumbnails_round_trip_and_hostile_files_are_none() {
        let d = tmp("th");
        let art = d.join("a.pcraft");
        let mut b = MAGIC.to_vec();
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let _ = std::fs::write(thumb_path(&art), &b);
        assert_eq!(read_thumb(&art).map(|i| i.size), Some([2, 1]));
        let _ = std::fs::write(thumb_path(&art), b"PCG1\xff\xff\xff\xff\xff\xff\xff\xffxx");
        assert!(read_thumb(&art).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
