//! Gallery: the artworks of the device as albums (folders), for phones and tablets.
//!
//! One root folder (the app's documents folder) holds albums; an album holds `.pcraft`
//! artworks. From here a person opens, creates, names and renames artworks, makes albums,
//! moves artworks between them, deletes them, and plays or exports an artwork's timelapse
//! ([`crate::timelapse`]). The file operations are plain functions ([`list`], [`rename`],
//! [`move_into`] …) so they are tested without a window; an artwork's timelapse folder always
//! travels with it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Align, Color32, Layout, RichText, Sense, TextureHandle, Vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::timelapse;

/// The artwork file extension.
pub const EXT: &str = "pcraft";

/// Canvas sizes offered for a new artwork: label, width, height (pixels).
pub const PRESETS: [(&str, u32, u32); 5] = [("Square 2048", 2048, 2048), ("Portrait 1536×2048", 1536, 2048), ("Landscape 2048×1536", 2048, 1536), ("HD 1920×1080", 1920, 1080), ("A4 @300 · 2480×3508", 2480, 3508)];

/// One thing in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// Albums first, then artworks, each A–Z. Hidden entries and timelapse folders are left out.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let file = path.file_name()?.to_string_lossy().into_owned();
            if file.starts_with('.') || file.ends_with(".timelapse") {
                return None;
            }
            let is_dir = path.is_dir();
            if !is_dir && path.extension().is_none_or(|x| x != EXT) {
                return None;
            }
            let name = if is_dir { file } else { path.file_stem()?.to_string_lossy().into_owned() };
            Some(Entry { path, name, is_dir })
        })
        .collect();
    v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
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

/// `name` made unique inside `dir` (adds " 2", " 3"…); `ext` is `""` for albums.
pub fn unique(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let file = |n: &str| if ext.is_empty() { dir.join(n) } else { dir.join(format!("{n}.{ext}")) };
    let mut p = file(name);
    let mut i = 2;
    while p.exists() && i < 10_000 {
        p = file(&format!("{name} {i}"));
        i += 1;
    }
    p
}

/// Make an album.
pub fn create_album(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = clean_name(name).ok_or("Type a name")?;
    let p = unique(dir, &name, "");
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    Ok(p)
}

fn move_sidecar(from: &Path, to: &Path) {
    let (a, b) = (timelapse::dir_for(from), timelapse::dir_for(to));
    if a.is_dir() {
        let _ = std::fs::rename(a, b);
    }
}

/// Rename an artwork or album; an artwork's timelapse follows.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let name = clean_name(new_name).ok_or("Type a name")?;
    let dir = path.parent().ok_or("No folder")?;
    let is_dir = path.is_dir();
    let target = if is_dir { dir.join(&name) } else { dir.join(format!("{name}.{EXT}")) };
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        return Err(format!("“{name}” already exists here"));
    }
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    if !is_dir {
        move_sidecar(path, &target);
    }
    Ok(target)
}

/// Move an artwork into an album (or back to the root); its timelapse follows.
pub fn move_into(path: &Path, album: &Path) -> Result<PathBuf, String> {
    let file = path.file_name().ok_or("No name")?;
    if path.is_dir() && album.starts_with(path) {
        return Err("An album can't go inside itself".into());
    }
    let target = album.join(file);
    if target == path {
        return Ok(target);
    }
    if target.exists() {
        return Err("Something with that name is already there".into());
    }
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    if !path.is_dir() {
        move_sidecar(path, &target);
    }
    Ok(target)
}

/// Delete an artwork (with its timelapse) or an album (with everything in it).
pub fn delete(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        let _ = std::fs::remove_dir_all(timelapse::dir_for(path));
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

/// Every album under `root` (and the root itself), for the Move picker.
pub fn albums(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in list(&d).into_iter().filter(|e| e.is_dir) {
            out.push(e.path.clone());
            if out.len() < 500 {
                stack.push(e.path);
            }
        }
    }
    out
}

/// What the person is doing to one entry.
#[derive(Clone, Debug)]
pub enum Edit {
    Rename(PathBuf, String),
    Move(PathBuf),
    Delete(PathBuf),
}

/// A timelapse being played.
pub struct Player {
    pub artwork: PathBuf,
    pub files: Vec<PathBuf>,
    pub pos: usize,
    pub playing: bool,
    pub fps: f32,
    t: f64,
    tex: Option<TextureHandle>,
    shown: Option<usize>,
}

/// Gallery state.
#[derive(Default)]
pub struct Gallery {
    /// The screen is showing (compact layout, or File ▸ Gallery).
    pub open: bool,
    /// Root folder of all artworks. Set by the host (Android: the app's documents folder).
    pub root: Option<PathBuf>,
    /// The album being looked at.
    pub cwd: Option<PathBuf>,
    pub edit: Option<Edit>,
    pub new_album: Option<String>,
    pub new_art: bool,
    pub new_name: String,
    pub status: Option<String>,
    pub player: Option<Player>,
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
}

impl Gallery {
    /// The root, falling back to `~/PhotoCraft` (created on demand).
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
        let tex = timelapse::frame_files(&timelapse::dir_for(art)).last().and_then(|p| timelapse::read_frame(p)).map(|f| ctx.load_texture(format!("thumb-{}", art.display()), f.to_color_image(), egui::TextureOptions::LINEAR));
        self.thumbs.insert(art.to_path_buf(), tex.clone());
        tex
    }

    /// Forget cached thumbnails (after a save, rename or delete).
    pub fn refresh(&mut self) {
        self.thumbs.clear();
    }
}

/// Draw the gallery over the whole window. Returns `true` while it is showing.
pub fn show(app: &mut PhotocraftApp, ui: &mut egui::Ui) -> bool {
    if !app.gallery.open {
        return false;
    }
    let ctx = ui.ctx().clone();
    let root = app.gallery.root();
    let cwd = app.gallery.cwd.clone().filter(|p| p.starts_with(&root) && p.is_dir()).unwrap_or_else(|| root.clone());
    let mut go: Option<PathBuf> = None;
    let mut open: Option<PathBuf> = None;
    let mut ask: Option<Edit> = None;
    let mut play: Option<PathBuf> = None;
    let mut gif: Option<PathBuf> = None;

    ui.horizontal(|ui| {
        if cwd != root && ui.add(egui::Button::new("‹ Back").min_size(Vec2::new(64.0, 40.0))).clicked() {
            go = cwd.parent().map(Path::to_path_buf);
        }
        let title = if cwd == root { "Gallery".to_string() } else { cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
        ui.heading(title);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(egui::Button::new("＋ Artwork").min_size(Vec2::new(96.0, 40.0))).clicked() {
                app.gallery.new_art = true;
                app.gallery.new_name = "Untitled".into();
            }
            if ui.add(egui::Button::new("📁 Album").min_size(Vec2::new(88.0, 40.0))).clicked() {
                app.gallery.new_album = Some(String::new());
            }
            if !app.session.documents().is_empty() && ui.add(egui::Button::new("Canvas").min_size(Vec2::new(72.0, 40.0))).clicked() {
                app.gallery.open = false;
            }
        });
    });
    if let Some(s) = app.gallery.status.clone() {
        ui.label(RichText::new(s).small());
    }
    ui.separator();

    let entries = list(&cwd);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if entries.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| ui.label("Nothing here yet. Tap ＋ Artwork to start drawing."));
        }
        let card = 150.0_f32;
        let cols = ((ui.available_width() / (card + 12.0)).floor() as usize).max(1);
        egui::Grid::new("gallery-grid").spacing([12.0, 12.0]).show(ui, |ui| {
            for (i, e) in entries.iter().enumerate() {
                ui.vertical(|ui| {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(card), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(rect, 8.0, Color32::from_gray(45));
                    if e.is_dir {
                        p.text(rect.center(), egui::Align2::CENTER_CENTER, "📁", egui::FontId::proportional(54.0), Color32::from_gray(200));
                    } else if let Some(t) = app.gallery.thumb(&ctx, &e.path) {
                        let s = t.size_vec2();
                        let k = (card - 12.0) / s.x.max(s.y).max(1.0);
                        p.image(t.id(), egui::Rect::from_center_size(rect.center(), s * k), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    } else {
                        p.text(rect.center(), egui::Align2::CENTER_CENTER, "🖼", egui::FontId::proportional(48.0), Color32::from_gray(150));
                    }
                    if resp.clicked() {
                        if e.is_dir {
                            go = Some(e.path.clone());
                        } else {
                            open = Some(e.path.clone());
                        }
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            ask = Some(Edit::Rename(e.path.clone(), e.name.clone()));
                            ui.close();
                        }
                        if ui.button("Move to album…").clicked() {
                            ask = Some(Edit::Move(e.path.clone()));
                            ui.close();
                        }
                        if !e.is_dir {
                            if ui.button("▶ Timelapse").clicked() {
                                play = Some(e.path.clone());
                                ui.close();
                            }
                            if ui.button("Export timelapse GIF").clicked() {
                                gif = Some(e.path.clone());
                                ui.close();
                            }
                        }
                        if ui.button("Delete").clicked() {
                            ask = Some(Edit::Delete(e.path.clone()));
                            ui.close();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([card - 34.0, 22.0], egui::Label::new(RichText::new(&e.name).strong()).truncate());
                        ui.menu_button("⋮", |ui| {
                            if ui.button("Rename").clicked() {
                                ask = Some(Edit::Rename(e.path.clone(), e.name.clone()));
                                ui.close();
                            }
                            if ui.button("Move to album…").clicked() {
                                ask = Some(Edit::Move(e.path.clone()));
                                ui.close();
                            }
                            if !e.is_dir {
                                if ui.button("▶ Timelapse").clicked() {
                                    play = Some(e.path.clone());
                                    ui.close();
                                }
                                if ui.button("Export timelapse GIF").clicked() {
                                    gif = Some(e.path.clone());
                                    ui.close();
                                }
                            }
                            if ui.button("Delete").clicked() {
                                ask = Some(Edit::Delete(e.path.clone()));
                                ui.close();
                            }
                        });
                    });
                });
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
    });

    if let Some(p) = go {
        app.gallery.cwd = Some(p);
    }
    if let Some(p) = open {
        match app.run("file.open", json!({ "path": p.to_string_lossy() })) {
            Ok(_) => {
                app.gallery.open = false;
                app.gallery.status = None;
            }
            Err(e) => app.gallery.status = Some(e),
        }
    }
    if let Some(e) = ask {
        app.gallery.edit = Some(e);
    }
    if let Some(p) = play {
        start_player(app, p);
    }
    if let Some(p) = gif {
        app.gallery.status = Some(match timelapse::export_gif(&p, 12) {
            Some(bytes) => {
                let out_dir = root.join("Exports");
                let _ = std::fs::create_dir_all(&out_dir);
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "timelapse".into());
                let out = unique(&out_dir, &format!("{name} timelapse"), "gif");
                match std::fs::write(&out, bytes) {
                    Ok(()) => format!("Saved {}", out.display()),
                    Err(e) => e.to_string(),
                }
            }
            None => "No timelapse was recorded for this artwork yet".into(),
        });
    }
    dialogs(app, &ctx, &cwd, &root);
    player(app, &ctx);
    true
}

fn start_player(app: &mut PhotocraftApp, artwork: PathBuf) {
    let files = timelapse::frame_files(&timelapse::dir_for(&artwork));
    if files.is_empty() {
        app.gallery.status = Some("No timelapse was recorded for this artwork yet".into());
        return;
    }
    app.gallery.player = Some(Player { artwork, files, pos: 0, playing: true, fps: 12.0, t: 0.0, tex: None, shown: None });
}

fn dialogs(app: &mut PhotocraftApp, ctx: &egui::Context, cwd: &Path, root: &Path) {
    // New album.
    if let Some(mut name) = app.gallery.new_album.clone() {
        let mut done = None;
        egui::Window::new("New album").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.text_edit_singleline(&mut name);
            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                app.gallery.status = create_album(cwd, &name).err();
                app.gallery.new_album = None;
            }
            Some(false) => app.gallery.new_album = None,
            None => app.gallery.new_album = Some(name),
        }
    }
    // New artwork: name and size.
    if app.gallery.new_art {
        let mut choose: Option<(u32, u32)> = None;
        let mut cancel = false;
        egui::Window::new("New artwork").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut app.gallery.new_name);
            ui.label("Canvas");
            for (label, w, h) in PRESETS {
                if ui.add(egui::Button::new(label).min_size(Vec2::new(220.0, 40.0))).clicked() {
                    choose = Some((w, h));
                }
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
        if let Some((w, h)) = choose {
            let name = clean_name(&app.gallery.new_name).unwrap_or_else(|| "Untitled".into());
            let path = unique(cwd, &name, EXT);
            let made = app.run("file.new", json!({ "width": w, "height": h })).and_then(|_| app.run("file.saveAs", json!({ "path": path.to_string_lossy() })));
            match made {
                Ok(_) => {
                    app.gallery.open = false;
                    app.gallery.status = None;
                    app.gallery.refresh();
                }
                Err(e) => app.gallery.status = Some(e),
            }
            app.gallery.new_art = false;
        } else if cancel {
            app.gallery.new_art = false;
        }
    }
    // Rename / move / delete.
    if let Some(edit) = app.gallery.edit.clone() {
        let mut next = Some(edit.clone());
        let mut result: Option<Result<(), String>> = None;
        match edit {
            Edit::Rename(path, mut name) => {
                egui::Window::new("Rename").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.text_edit_singleline(&mut name);
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() {
                            result = Some(rename(&path, &name).map(|_| ()));
                        }
                        if ui.button("Cancel").clicked() {
                            result = Some(Ok(()));
                        }
                    });
                });
                if result.is_none() {
                    next = Some(Edit::Rename(path, name));
                }
            }
            Edit::Move(path) => {
                egui::Window::new("Move to album").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for a in albums(root) {
                            if path.is_dir() && a.starts_with(&path) {
                                continue;
                            }
                            let label = if a == root { "Gallery (top level)".to_string() } else { a.strip_prefix(root).map(|p| p.display().to_string()).unwrap_or_default() };
                            if ui.add(egui::Button::new(label).min_size(Vec2::new(240.0, 36.0))).clicked() {
                                result = Some(move_into(&path, &a).map(|_| ()));
                            }
                        }
                    });
                    if ui.button("Cancel").clicked() {
                        result = Some(Ok(()));
                    }
                });
            }
            Edit::Delete(path) => {
                egui::Window::new("Delete").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Delete “{}”? This can't be undone.", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            result = Some(delete(&path));
                        }
                        if ui.button("Cancel").clicked() {
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

fn player(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(p) = app.gallery.player.as_mut() else { return };
    let mut close = false;
    if p.playing {
        p.t += f64::from(ctx.input(|i| i.stable_dt));
        let step = 1.0 / f64::from(p.fps.max(1.0));
        while p.t >= step {
            p.t -= step;
            if p.pos + 1 < p.files.len() {
                p.pos += 1;
            } else {
                p.playing = false;
            }
        }
        ctx.request_repaint();
    }
    if p.shown != Some(p.pos) {
        if let Some(f) = p.files.get(p.pos).and_then(|f| timelapse::read_frame(f)) {
            p.tex = Some(ctx.load_texture("timelapse-frame", f.to_color_image(), egui::TextureOptions::LINEAR));
        }
        p.shown = Some(p.pos);
    }
    let title = p.artwork.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    egui::Window::new(format!("Timelapse · {title}")).collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        if let Some(t) = &p.tex {
            let s = t.size_vec2();
            let k = (ui.available_width().min(560.0) / s.x.max(1.0)).min(1.0e3);
            ui.image((t.id(), s * k));
        }
        let last = p.files.len().saturating_sub(1);
        ui.horizontal(|ui| {
            if ui.button(if p.playing { "⏸" } else { "▶" }).clicked() {
                if !p.playing && p.pos >= last {
                    p.pos = 0;
                }
                p.playing = !p.playing;
            }
            let mut pos = p.pos;
            if ui.add(egui::Slider::new(&mut pos, 0..=last).show_value(false)).changed() {
                p.pos = pos;
                p.playing = false;
            }
        });
        ui.add(egui::Slider::new(&mut p.fps, 2.0..=60.0).text("fps"));
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if close {
        app.gallery.player = None;
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
    fn rename_and_move_carry_the_timelapse() {
        let d = tmp("mv");
        let art = d.join("a.pcraft");
        let _ = std::fs::write(&art, b"x");
        let tl = timelapse::dir_for(&art);
        let _ = std::fs::create_dir_all(&tl);
        let _ = std::fs::write(tl.join("00000.idx"), b"y");
        let renamed = rename(&art, "Moon").unwrap_or_default();
        assert!(renamed.ends_with("Moon.pcraft") && timelapse::dir_for(&renamed).join("00000.idx").exists());
        let album = create_album(&d, "Sketches").unwrap_or_default();
        let moved = move_into(&renamed, &album).unwrap_or_default();
        assert!(moved.exists() && timelapse::dir_for(&moved).join("00000.idx").exists());
        assert!(delete(&moved).is_ok() && !timelapse::dir_for(&moved).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn list_puts_albums_first_and_hides_timelapse_folders() {
        let d = tmp("ls");
        let _ = std::fs::write(d.join("b.pcraft"), b"x");
        let _ = std::fs::create_dir_all(d.join("b.pcraft.timelapse"));
        let _ = std::fs::create_dir_all(d.join("Zed"));
        let _ = std::fs::write(d.join("notes.txt"), b"x");
        let names: Vec<_> = list(&d).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Zed", "b"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_album_cannot_move_into_itself() {
        let d = tmp("self");
        let a = create_album(&d, "A").unwrap_or_default();
        let inner = create_album(&a, "B").unwrap_or_default();
        assert!(move_into(&a, &inner).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
