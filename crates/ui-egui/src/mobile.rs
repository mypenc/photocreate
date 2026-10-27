//! Phone and tablet layout. Everything the desktop app has stays: the full menu bar, the tool
//! options bar, every tool and every panel. Only the chrome is arranged for fingers:
//!
//! * the **menu bar** keeps all its menus and scrolls sideways when they do not fit;
//! * the **options bar** stays under it, as on the desktop;
//! * a **bottom bar** puts what a painter reaches for to the thumb: pen and eraser, the brush
//!   settings, the colour, layers, undo and redo, the full tool picker and the panel dock;
//! * the left tool column and the status bar go; tools open from the bottom bar;
//! * panels (Layers, Color, Brush Settings and the rest) open in the dock as they do on the
//!   desktop, from the bottom bar or the Window menu.
//!
//! The mode follows the window width on a touch device: under [`PHONE_MAX_WIDTH`] points is a
//! phone, wider is a tablet. A preference can force either, or the desktop layout.

use egui::{Color32, Sense, Vec2};
use serde_json::json;

use crate::PhotocraftApp;
use crate::icons;
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

/// The layout in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Tablet,
    Phone,
}

/// Which layout a window of `width` points gets. `touch` is true on a touch device or once a
/// touchscreen has been used.
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

/// Mobile layout state.
#[derive(Default)]
pub struct State {
    pub pref: Pref,
    /// The tool picker is open.
    pub tools_open: bool,
    /// The panel dock is showing beside the canvas.
    pub dock_open: bool,
}

impl PhotocraftApp {
    /// The layout this frame uses.
    pub fn mobile_mode(&self, ctx: &egui::Context) -> Mode {
        mode_for(self.mobile.pref, ctx.content_rect().width(), self.touch.enabled || cfg!(any(target_os = "android", target_os = "ios")))
    }
}

/// The menu bar, scrolling sideways when its menus do not fit.
pub fn menu_row(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    egui::Panel::top("mobile-menu").show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("mobile-menu-scroll").auto_shrink([false, true]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.set_min_height(36.0);
            crate::menus::menu_bar(app, ui);
        });
    });
}

fn open_panel(app: &mut PhotocraftApp, id: &str) {
    let _ = app.run(id, json!({}));
    app.mobile.dock_open = true;
    app.ui.panels.dock = true;
}

fn tool_button(ui: &mut egui::Ui, app: &mut PhotocraftApp, tool: Tool, size: f32) {
    let on = app.ui.tool == tool;
    if icons::button(ui, icons::tool_icon(tool), size, on, &format!("{tool:?}")).clicked() {
        app.ui.tool = tool;
    }
}

/// The bottom bar.
pub fn bottom_bar(app: &mut PhotocraftApp, ui: &mut egui::Ui, mode: Mode) {
    let size = if mode == Mode::Phone { 44.0 } else { 52.0 };
    egui::Panel::bottom("mobile-bottom").show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("mobile-bottom-scroll").auto_shrink([false, true]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if icons::button(ui, "folder", size, app.gallery.open, "Gallery").clicked() {
                    app.gallery.open = !app.gallery.open;
                    app.gallery.refresh();
                }
                ui.separator();
                // Pen and eraser.
                tool_button(ui, app, Tool::Brush, size);
                tool_button(ui, app, Tool::Eraser, size);
                ui.separator();
                // Brush: type and settings.
                if icons::button(ui, "brush", size, false, "Brushes").clicked() {
                    open_panel(app, "window.panel.brushes");
                }
                if icons::button(ui, "sliders-horizontal", size, false, "Brush settings").clicked() {
                    open_panel(app, "window.panel.brushSettings");
                }
                // Colour: the swatch is the foreground colour.
                let fg = app.session.tools.foreground;
                let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
                ui.painter().rect_filled(rect.shrink(5.0), 8.0, Color32::from_rgb(c(fg[0]), c(fg[1]), c(fg[2])));
                ui.painter().rect_stroke(rect.shrink(5.0), 8.0, egui::Stroke::new(1.5, Color32::GRAY), egui::StrokeKind::Inside);
                if resp.on_hover_text("Color").clicked() {
                    open_panel(app, "window.panel.color");
                }
                if icons::button(ui, "layers", size, false, "Layers").clicked() {
                    open_panel(app, "window.panel.layers");
                }
                ui.separator();
                if icons::button(ui, "undo-2", size, false, "Undo").clicked() {
                    let _ = app.run("edit.undo", json!({}));
                }
                if icons::button(ui, "redo-2", size, false, "Redo").clicked() {
                    let _ = app.run("edit.redo", json!({}));
                }
                ui.separator();
                if icons::button(ui, "grid-3x3", size, app.mobile.tools_open, "All tools").clicked() {
                    app.mobile.tools_open = !app.mobile.tools_open;
                }
                if icons::button(ui, "panel-right", size, app.mobile.dock_open, "Panels").clicked() {
                    app.mobile.dock_open = !app.mobile.dock_open;
                    app.ui.panels.dock = app.mobile.dock_open;
                }
            });
        });
    });
}

/// Every tool, in a grid above the bottom bar.
pub fn tools_window(app: &mut PhotocraftApp, ctx: &egui::Context, mode: Mode) {
    if !app.mobile.tools_open {
        return;
    }
    let size = if mode == Mode::Phone { 44.0 } else { 52.0 };
    let mut picked = false;
    egui::Window::new("Tools").collapsible(false).resizable(false).anchor(egui::Align2::LEFT_BOTTOM, [8.0, -(size + 28.0)]).show(ctx, |ui| {
        let cols = (((ctx.content_rect().width() - 40.0) / (size + 8.0)).floor() as usize).clamp(3, 8);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() * 0.5).max(160.0)).show(ui, |ui| {
            egui::Grid::new("mobile-tools").spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, tool) in Tool::ALL.iter().copied().enumerate() {
                    let before = app.ui.tool;
                    tool_button(ui, app, tool, size);
                    picked |= app.ui.tool != before;
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    });
    if picked {
        app.mobile.tools_open = false;
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
}
