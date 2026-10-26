//! Phone and tablet layout, modelled on mobile painting apps.
//!
//! The desktop window (menu bar, options bar, toolbox, docked panels) leaves a pad or phone a
//! thin strip to paint on. In the compact layouts the chrome is rebuilt for fingers and a pen,
//! and the canvas gets the whole screen:
//!
//! * **top bar**: Gallery, the artwork's name (`*` when unsaved), Undo, Redo, Save, Menu;
//! * **left edge**: two vertical sliders, brush size over opacity, under a thumb;
//! * **tool rail** down the right edge: Brush, Eraser, Smudge, Eyedropper, Fill, Select, Move,
//!   Text, Hand and Rotate, then Color and Layers, each opening a flyout beside the rail;
//! * **canvas**: the document area itself. One finger or the pen paints, two fingers pan, pinch
//!   zooms and twist rotates ([`crate::touch`]); two quick taps undo.
//!
//! The mode follows the window width unless the preference forces one: under
//! [`PHONE_MAX_WIDTH`] points is a phone (a slimmer rail, the flyouts cover the canvas), wider is
//! a tablet.

use egui::{Align, Color32, Layout, RichText, Sense, Vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::canvas;
use crate::state::Tool;

/// Widest window (points) still laid out as a phone.
pub const PHONE_MAX_WIDTH: f32 = 600.0;

/// The person's choice of layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pref {
    /// Phone or tablet by width on a touch device, desktop otherwise.
    #[default]
    Auto,
    Desktop,
    Tablet,
    Phone,
}

impl Pref {
    pub fn as_str(self) -> &'static str {
        match self {
            Pref::Auto => "auto",
            Pref::Desktop => "desktop",
            Pref::Tablet => "tablet",
            Pref::Phone => "phone",
        }
    }

    pub fn parse(s: &str) -> Option<Pref> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(Pref::Auto),
            "desktop" => Some(Pref::Desktop),
            "tablet" | "pad" => Some(Pref::Tablet),
            "phone" => Some(Pref::Phone),
            _ => None,
        }
    }
}

/// The layout in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Tablet,
    Phone,
}

/// Which layout a window of `width` points gets. `touch` is true on a touch device (Android,
/// iOS) or once a touchscreen has been used.
pub fn mode_for(pref: Pref, width: f32, touch: bool) -> Mode {
    match pref {
        Pref::Desktop => Mode::Desktop,
        Pref::Tablet => Mode::Tablet,
        Pref::Phone => Mode::Phone,
        Pref::Auto if !touch => Mode::Desktop,
        Pref::Auto if width < PHONE_MAX_WIDTH => Mode::Phone,
        Pref::Auto => Mode::Tablet,
    }
}

/// A flyout beside the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flyout {
    Color,
    Layers,
}

/// Compact layout state.
#[derive(Default)]
pub struct State {
    pub pref: Pref,
    pub flyout: Option<Flyout>,
    /// The ⋯ menu is open.
    pub menu: bool,
}

/// The rail's tools: label, glyph, tool.
const TOOLS: [(&str, &str, Tool); 10] = [
    ("Brush", "🖌", Tool::Brush),
    ("Eraser", "⌫", Tool::Eraser),
    ("Smudge", "☝", Tool::Smudge),
    ("Pick", "💧", Tool::Eyedropper),
    ("Fill", "🪣", Tool::PaintBucket),
    ("Select", "⬚", Tool::Lasso),
    ("Move", "✥", Tool::Move),
    ("Text", "T", Tool::Type),
    ("Hand", "✋", Tool::Hand),
    ("Rotate", "⟳", Tool::RotateView),
];

fn big(ui: &mut egui::Ui, label: &str, size: Vec2, on: bool) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).size(18.0)).min_size(size).selected(on))
}

/// Lay out the whole window in a compact mode.
pub fn draw(app: &mut PhotocraftApp, ui: &mut egui::Ui, mode: Mode) {
    let phone = mode == Mode::Phone;
    let btn = Vec2::splat(if phone { 40.0 } else { 46.0 });
    let ctx = ui.ctx().clone();
    let has_doc = !app.session.documents().is_empty();
    if !has_doc {
        app.gallery.open = true;
    }

    // Top bar.
    egui::Panel::top("compact-top").show(ui, |ui| {
        ui.horizontal(|ui| {
            if big(ui, "▦", btn, app.gallery.open).on_hover_text("Gallery").clicked() {
                app.gallery.open = !app.gallery.open;
                app.gallery.refresh();
            }
            let name = app.session.active().map(|st| format!("{}{}", st.doc.name, if st.revision != st.saved_revision { " *" } else { "" })).unwrap_or_default();
            ui.add(egui::Label::new(RichText::new(name).strong()).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if big(ui, "⋯", btn, false).clicked() {
                    app.compact.menu = !app.compact.menu;
                }
                if has_doc && big(ui, "💾", btn, false).on_hover_text("Save").clicked() {
                    if let Err(e) = app.run("file.save", json!({})) {
                        app.gallery.status = Some(e);
                    } else {
                        app.gallery.refresh();
                    }
                }
                if has_doc && big(ui, "↷", btn, false).on_hover_text("Redo").clicked() {
                    let _ = app.run("edit.redo", json!({}));
                }
                if has_doc && big(ui, "↶", btn, false).on_hover_text("Undo").clicked() {
                    let _ = app.run("edit.undo", json!({}));
                }
            });
        });
    });

    if app.compact.menu {
        egui::Window::new("Menu").collapsible(false).resizable(false).anchor(egui::Align2::RIGHT_TOP, [-8.0, 56.0]).show(&ctx, |ui| {
            let mut rec = app.timelapse.enabled;
            if ui.checkbox(&mut rec, "Record timelapse").changed() {
                app.timelapse.enabled = rec;
            }
            let mut dt = app.touch.double_tap_undo;
            if ui.checkbox(&mut dt, "Double-tap canvas to undo").changed() {
                app.touch.double_tap_undo = dt;
            }
            ui.separator();
            for (label, pref) in [("Auto layout", Pref::Auto), ("Tablet layout", Pref::Tablet), ("Phone layout", Pref::Phone), ("Desktop layout", Pref::Desktop)] {
                if ui.radio(app.compact.pref == pref, label).clicked() {
                    app.compact.pref = pref;
                }
            }
            if ui.button("Close").clicked() {
                app.compact.menu = false;
            }
        });
    }

    // The gallery replaces the canvas while it is open.
    if crate::gallery::show(app, ui) {
        return;
    }

    // Right rail.
    egui::Panel::right("compact-rail").resizable(false).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (label, glyph, tool) in TOOLS {
                let r = big(ui, glyph, btn, app.ui.tool == tool).on_hover_text(label);
                if r.clicked() {
                    app.ui.tool = tool;
                    app.compact.flyout = None;
                }
                if !phone {
                    ui.label(RichText::new(label).small());
                }
            }
            ui.separator();
            let fg = app.session.tools.foreground;
            let swatch = Color32::from_rgb((fg[0].clamp(0.0, 1.0) * 255.0) as u8, (fg[1].clamp(0.0, 1.0) * 255.0) as u8, (fg[2].clamp(0.0, 1.0) * 255.0) as u8);
            let (rect, resp) = ui.allocate_exact_size(btn, Sense::click());
            ui.painter().rect_filled(rect.shrink(4.0), 6.0, swatch);
            ui.painter().rect_stroke(rect.shrink(4.0), 6.0, egui::Stroke::new(1.5, Color32::GRAY), egui::StrokeKind::Inside);
            if resp.on_hover_text("Color").clicked() {
                app.compact.flyout = if app.compact.flyout == Some(Flyout::Color) { None } else { Some(Flyout::Color) };
            }
            if big(ui, "☰", btn, app.compact.flyout == Some(Flyout::Layers)).on_hover_text("Layers").clicked() {
                app.compact.flyout = if app.compact.flyout == Some(Flyout::Layers) { None } else { Some(Flyout::Layers) };
            }
        });
    });

    // Flyout beside the rail.
    if let Some(f) = app.compact.flyout {
        let width = if phone { 240.0 } else { 300.0 };
        egui::Panel::right("compact-flyout").exact_size(width).resizable(false).show(ui, |ui| match f {
            Flyout::Color => color_flyout(app, ui),
            Flyout::Layers => layers_flyout(app, ui),
        });
    }

    // Left sliders: brush size over opacity.
    egui::Panel::left("compact-sliders").resizable(false).exact_size(if phone { 44.0 } else { 56.0 }).show(ui, |ui| {
        let h = (ui.available_height() / 2.0 - 36.0).clamp(80.0, 320.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("{:.0}", app.session.tools.brush.size)).small());
            let mut size = app.session.tools.brush.size.max(1.0).sqrt();
            let max = photocraft_paint::MAX_BRUSH_SIZE.sqrt();
            ui.spacing_mut().slider_width = h;
            // Square-root scale: fine control among small sizes, reach for large ones.
            if ui.add(egui::Slider::new(&mut size, 1.0..=max).vertical().show_value(false)).changed() {
                app.session.tools.brush.size = (size * size).clamp(1.0, photocraft_paint::MAX_BRUSH_SIZE);
            }
            ui.add_space(8.0);
            ui.label(RichText::new(format!("{:.0}%", app.session.tools.brush.opacity * 100.0)).small());
            let mut op = app.session.tools.brush.opacity.clamp(0.0, 1.0);
            if ui.add(egui::Slider::new(&mut op, 0.01..=1.0).vertical().show_value(false)).changed() {
                app.session.tools.brush.opacity = op;
            }
        });
    });

    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        canvas::document_area(app, ui);
    });
}

fn color_flyout(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    ui.heading("Color");
    let fg = app.session.tools.foreground;
    let mut rgb = [fg[0], fg[1], fg[2]];
    ui.spacing_mut().slider_width = 200.0;
    if egui::color_picker::color_edit_button_rgb(ui, &mut rgb).changed() {
        app.session.tools.foreground = [rgb[0], rgb[1], rgb[2], fg[3]];
    }
    for (i, name) in ["R", "G", "B"].iter().enumerate() {
        let mut v = rgb[i];
        if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).text(*name)).changed() {
            let mut c = app.session.tools.foreground;
            c[i] = v;
            app.session.tools.foreground = c;
        }
    }
    ui.separator();
    ui.label("Swatches");
    ui.horizontal_wrapped(|ui| {
        for c in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.9, 0.2, 0.2], [0.95, 0.6, 0.15], [0.95, 0.85, 0.2], [0.3, 0.7, 0.3], [0.2, 0.5, 0.9], [0.5, 0.3, 0.8], [0.9, 0.5, 0.7], [0.5, 0.35, 0.2]] {
            let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
            ui.painter().rect_filled(r.shrink(2.0), 6.0, col);
            if resp.clicked() {
                app.session.tools.foreground = [c[0], c[1], c[2], 1.0];
            }
        }
    });
}

fn layers_flyout(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Layers");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(egui::Button::new("＋").min_size(Vec2::splat(36.0))).on_hover_text("New layer").clicked() {
                let _ = app.run("layer.new.layer", json!({}));
            }
            if ui.add(egui::Button::new("⧉").min_size(Vec2::splat(36.0))).on_hover_text("Duplicate").clicked() {
                let _ = app.run("layer.duplicate", json!({}));
            }
            if ui.add(egui::Button::new("🗑").min_size(Vec2::splat(36.0))).on_hover_text("Delete").clicked() {
                let _ = app.run("layer.delete", json!({}));
            }
        });
    });
    let Some(st) = app.session.active() else { return };
    // Top layer first.
    let rows: Vec<(u64, String, bool, bool)> = st.doc.layers.iter().rev().map(|l| (l.id.0, l.name.clone(), l.visible, st.active_layer == Some(l.id))).collect();
    let mut select = None;
    let mut toggle = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (id, name, visible, active) in &rows {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(if *visible { "👁" } else { "–" }).min_size(Vec2::splat(36.0))).clicked() {
                    toggle = Some(*id);
                }
                if ui.add(egui::Button::new(name).min_size(Vec2::new(ui.available_width(), 36.0)).selected(*active)).clicked() {
                    select = Some(*id);
                }
            });
        }
    });
    if let Some(id) = select {
        let _ = app.run("layer.select", json!({ "layer": id, "mode": "replace" }));
    }
    if let Some(id) = toggle {
        let _ = app.run("layer.hideLayers", json!({ "layer": id }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_follows_width_on_touch_only() {
        assert_eq!(mode_for(Pref::Auto, 400.0, true), Mode::Phone);
        assert_eq!(mode_for(Pref::Auto, 900.0, true), Mode::Tablet);
        assert_eq!(mode_for(Pref::Auto, 400.0, false), Mode::Desktop);
        assert_eq!(mode_for(Pref::Phone, 1600.0, false), Mode::Phone);
        assert_eq!(mode_for(Pref::Desktop, 400.0, true), Mode::Desktop);
    }

    #[test]
    fn pref_round_trips() {
        for p in [Pref::Auto, Pref::Desktop, Pref::Tablet, Pref::Phone] {
            assert_eq!(Pref::parse(p.as_str()), Some(p));
        }
        assert_eq!(Pref::parse("nope"), None);
    }
}
