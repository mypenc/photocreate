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
use crate::dock::Group;
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
    /// Panels open as floating windows, oldest first (the last one is on top).
    pub open: Vec<Group>,
    /// The list of all panels is open.
    pub panel_list: bool,
    /// The desktop layout's default panels were cleared once.
    started: bool,
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

/// Open `g` as a floating window (or bring it back to the top).
pub fn open_group(app: &mut PhotocraftApp, g: Group) {
    app.mobile.open.retain(|x| *x != g);
    app.mobile.open.push(g);
}

/// Panels the Window menu or a command asked for arrive as "shown" in the desktop dock, which
/// the mobile layout does not draw: they float instead.
fn take_reveals(app: &mut PhotocraftApp) {
    let first = !app.mobile.started;
    app.mobile.started = true;
    for g in Group::ALL {
        if g.shown(&app.ui.panels) {
            *g.shown_mut(&mut app.ui.panels) = false;
            if !first {
                open_group(app, g);
            }
        }
    }
}

fn group_title(g: Group) -> &'static str {
    match g {
        Group::Color => "Color",
        Group::Properties => "Properties",
        Group::Character => "Text",
        Group::Navigator => "Navigator",
        Group::History => "History",
        Group::Layers => "Layers",
        Group::Brushes => "Brushes",
    }
}

/// The floating panels, like a paint app's: each is its own window over the canvas, with its
/// tabs along the top and a big close button. Drag the title row to move, the corner to resize.
pub fn floating_panels(app: &mut PhotocraftApp, ctx: &egui::Context, mode: Mode) {
    take_reveals(app);
    let screen = ctx.content_rect();
    let width = if mode == Mode::Phone { (screen.width() - 16.0).clamp(240.0, 380.0) } else { 360.0 };
    let height = (screen.height() * if mode == Mode::Phone { 0.5 } else { 0.62 }).clamp(220.0, 560.0);
    let pro = crate::theme::Tokens::get(ctx).pro;
    let mut close: Option<Group> = None;
    let mut raise: Option<Group> = None;
    let open = app.mobile.open.clone();
    for (n, g) in open.iter().copied().enumerate() {
        let pos = if mode == Mode::Phone { egui::pos2(8.0, screen.top() + 96.0 + 14.0 * n as f32) } else { egui::pos2(screen.right() - width - 12.0, screen.top() + 96.0 + 28.0 * n as f32) };
        let resp = egui::Window::new(group_title(g))
            .id(egui::Id::new(("mobile-panel", g.key())))
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_pos(pos)
            .default_size([width, height])
            .min_width(220.0)
            .constrain_to(screen)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // The title row is the handle: drag it to move the panel.
                    let tabs = app.ui.dock.visible_tabs(g, pro);
                    let sel = *g.tab_mut(&mut app.ui.dock_tabs);
                    let mut pick = None;
                    egui::ScrollArea::horizontal().id_salt(("mobile-tabs", g.key())).max_width(ui.available_width() - 52.0).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (i, name) in &tabs {
                                if ui.add(egui::Button::new(crate::i18n::tr(crate::i18n::current(), name)).min_size(egui::vec2(0.0, 36.0)).selected(*i == sel)).clicked() {
                                    pick = Some(*i);
                                }
                            }
                        });
                    });
                    if let Some(i) = pick {
                        *g.tab_mut(&mut app.ui.dock_tabs) = i;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, "x", 36.0, false, "Close").clicked() {
                            close = Some(g);
                        }
                    });
                });
                ui.separator();
                let tab = *g.tab_mut(&mut app.ui.dock_tabs);
                let body_h = (ui.available_height()).max(120.0);
                if g.scrolls_itself(tab) {
                    ui.set_min_height(body_h);
                    crate::panels::dock_body(app, ui, g, tab);
                } else {
                    ui.spacing_mut().scroll.fade.strength = 0.0;
                    egui::ScrollArea::vertical().id_salt(("mobile-body", g.key())).auto_shrink([false, false]).show(ui, |ui| crate::panels::dock_body(app, ui, g, tab));
                }
            });
        if let Some(r) = resp
            && r.response.clicked()
        {
            raise = Some(g);
        }
    }
    if let Some(g) = close {
        app.mobile.open.retain(|x| *x != g);
    } else if let Some(g) = raise
        && app.mobile.open.last() != Some(&g)
    {
        open_group(app, g);
    }
    panel_list(app, ctx);
}

/// A list of every panel, to open any of them.
fn panel_list(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.mobile.panel_list {
        return;
    }
    let mut picked = None;
    let mut close = false;
    egui::Window::new("Panels").collapsible(false).resizable(false).anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -72.0]).show(ctx, |ui| {
        for g in Group::ALL {
            if ui.add(egui::Button::new(group_title(g)).min_size(egui::vec2(180.0, 40.0)).selected(app.mobile.open.contains(&g))).clicked() {
                picked = Some(g);
            }
        }
        if ui.add(egui::Button::new("Brush settings").min_size(egui::vec2(180.0, 40.0))).clicked() {
            let _ = app.run("window.panel.brushSettings", json!({}));
            close = true;
        }
        if ui.add(egui::Button::new("Close").min_size(egui::vec2(180.0, 40.0))).clicked() {
            close = true;
        }
    });
    if let Some(g) = picked {
        open_group(app, g);
        app.mobile.panel_list = false;
    }
    if close {
        app.mobile.panel_list = false;
    }
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
                if icons::button(ui, "brush", size, app.mobile.open.contains(&Group::Brushes), "Brushes").clicked() {
                    open_group(app, Group::Brushes);
                }
                if icons::button(ui, "sliders-horizontal", size, false, "Brush settings").clicked() {
                    let _ = app.run("window.panel.brushSettings", json!({}));
                }
                // Colour: the swatch is the foreground colour.
                let fg = app.session.tools.foreground;
                let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
                ui.painter().rect_filled(rect.shrink(5.0), 8.0, Color32::from_rgb(c(fg[0]), c(fg[1]), c(fg[2])));
                ui.painter().rect_stroke(rect.shrink(5.0), 8.0, egui::Stroke::new(1.5, Color32::GRAY), egui::StrokeKind::Inside);
                if resp.on_hover_text("Color").clicked() {
                    open_group(app, Group::Color);
                    *Group::Color.tab_mut(&mut app.ui.dock_tabs) = app.ui.dock.visible_tabs(Group::Color, crate::theme::Tokens::get(ui.ctx()).pro).iter().find(|(_, n)| *n == "Color").map_or(0, |(i, _)| *i);
                }
                if icons::button(ui, "layers", size, app.mobile.open.contains(&Group::Layers), "Layers").clicked() {
                    open_group(app, Group::Layers);
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
                if icons::button(ui, "panel-right", size, app.mobile.panel_list, "All panels").clicked() {
                    app.mobile.panel_list = !app.mobile.panel_list;
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
