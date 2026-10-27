//! PhotoCreate for Android.
//!
//! The same engine and egui interface as the desktop app (`apps/photocreate`), compiled as a
//! `cdylib` that a GameActivity (see `android/`) loads. There is no web view: eframe draws with
//! wgpu (Vulkan, OpenGL ES fallback) straight into the activity's surface.
//!
//! What differs from the desktop app:
//!
//! * **Layout.** Phones and pads get the compact layout (`photocraft_ui_egui::compact`): a menu
//!   button, a touch-sized tool bar, a fixed bottom dock of drafting toggles, and Layers /
//!   Properties as a side panel or sheet. The floating touch bar is off by default (View ▸ Touch).
//! * **Files.** Android hands apps `content://` URIs, not paths. The pickers go to Kotlin
//!   ([`bridge`]): Open copies the drawing into the app's folder; Save writes there and
//!   `Exports.kt` mirrors it to the file the user named. The pickers never block this thread
//!   (see [`bridge::pick`]): the command that asked is run again when the user has chosen.
//! * **Input.** Fingers become mouse events in `photocraft_ui_egui::touch` (long press = right
//!   click, two fingers pan, pinch zooms, drag = window selection); a mouse and keyboard reach
//!   egui through the patched winit (`android/patches/`).
//! * **Crashes.** If the app dies before its first frame, the next start shows the crash log with
//!   Try again / Copy / Exit (`DebugLog.kt`). Drawings are autosaved and offered for recovery.
//! * **No** control server, SpaceMouse, native menus, print or drag and drop.
//!
//! Everything here is `cfg(target_os = "android")`; on other targets the crate is empty.
#![cfg(target_os = "android")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[allow(dead_code)]
mod bridge;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use android_activity::AndroidApp;
use android_activity::input::{Axis, ImeOptions, InputType, TextInputAction};
use photocraft_engine::Session;
use photocraft_ui_egui::PhotocraftApp;

mod mobile_services;

/// Stencil bits of the window's render pass (see `NativeOptions::stencil_buffer`).
const STENCIL_BITS: u8 = 8;

const PREFS_EVERY: Duration = Duration::from_secs(3);

/// Entry point: android-activity calls this on its own thread, not the Java UI thread.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    prepare_environment(&app);
    bridge::set_loading_progress(&app, 10);
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("PhotoCreate"));
    install_panic_hook();
    // Mouse wheel axes are off by default in GameActivity.
    app.enable_motion_axis(Axis::Hscroll);
    app.enable_motion_axis(Axis::Vscroll);
    bridge::set_loading_progress(&app, 35);

    let options = eframe::NativeOptions {
        android_app: Some(app.clone()),
        wgpu_options: wgpu_options(),
        stencil_buffer: STENCIL_BITS,
        ..Default::default()
    };
    let host = app.clone();
    let result = eframe::run_native(
        "PhotoCreate",
        options,
        Box::new(move |cc| {
            match &cc.wgpu_render_state {
                Some(rs) => log::info!("graphics: {:?}", rs.adapter.get_info()),
                None => log::info!("graphics: no wgpu device"),
            }
            bridge::set_context(cc.egui_ctx.clone());
            PhotocraftApp::install_type_reset_hooks(&cc.egui_ctx);
            let mut cad = PhotocraftApp::new(Session::new(), mobile_services::mobile());
            cad.touch.enabled = true;
            cad.background_jobs = false;
            // Artworks live in the app's own storage: <files>/Artworks, albums are its folders.
            if let Some(data) = host.internal_data_path() {
                let root = data.join("Artworks");
                let _ = std::fs::create_dir_all(&root);
                cad.gallery.root = Some(root);
            }
            cad.gallery.open = true;
            bridge::set_loading_progress(&host, 80);
            host.set_ime_editor_info(
                InputType::from_bits_retain(0x1 | 0x4000 | 0x2_0000 | 0x8_0000),
                TextInputAction::None,
                ImeOptions::from_bits_retain(0x4000_0000 | 0x1000_0000),
            );
            let h = host.clone();
            cad.touch.haptic = Some(Box::new(move || bridge::haptic(&h)));
            Ok(Box::new(AndroidHost { cad, android: host, last_copied: String::new(), fullscreen_applied: false, loading_hidden: false }))
        }),
    );
    if let Err(e) = result {
        log::error!("PhotoCreate could not start: {e}");
    }
}

/// A panic goes to the log file and logcat with a backtrace before anything else happens, so the
/// crash dialog of the next start has something to show.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC on thread {:?}: {info}\n{backtrace}", std::thread::current().name());
        previous(info);
    }));
}

/// `$HOME`, the temp folder and the settings folder must point inside the app's own storage;
/// the desktop defaults (`~/.config`, `/data/local/tmp`) don't exist or aren't writable here.
fn prepare_environment(app: &AndroidApp) {
    let Some(data) = app.internal_data_path() else { return };
    let tmp = data.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let config = data.join("config");
    let _ = std::fs::create_dir_all(&config);
    // SAFETY: first thing `android_main` does, before this app spawns any thread of its own.
    unsafe {
        std::env::set_var("HOME", &data);
        std::env::set_var("TMPDIR", &tmp);
        std::env::set_var("PHOTOCREATE_CONFIG_DIR", &config);
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}

/// The settings folder (`<files>/config`): logs, autosave, LISP support, preferences.
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("PHOTOCREATE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Ask the adapter for its own limits (a modest phone GPU still opens the app, and a large
/// preview's index buffer fits). Vulkan first, OpenGL ES as the fallback.
fn wgpu_options() -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::WgpuSetup;
    let mut config = eframe::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN | eframe::wgpu::Backends::GL;
        setup.device_descriptor = Arc::new(|adapter| {
            let limits = adapter.limits();
            eframe::wgpu::DeviceDescriptor {
                label: Some("PhotoCreate device"),
                required_limits: eframe::wgpu::Limits {
                    max_texture_dimension_2d: limits.max_texture_dimension_2d.min(16384),
                    max_buffer_size: limits.max_buffer_size,
                    ..limits
                },
                ..Default::default()
            }
        });
    }
    config
}

struct AndroidHost {
    cad: PhotocraftApp,
    android: AndroidApp,
    /// The last text mirrored to the system clipboard (so an unchanged copy isn't sent twice).
    last_copied: String,
    fullscreen_applied: bool,
    loading_hidden: bool,
}

impl eframe::App for AndroidHost {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.cad, ctx, raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        for path in bridge::take_opened() {
            let p = path.clone();
            if let Err(e) = self.cad.run("file.open", serde_json::json!({ "path": p })) {
                self.cad.gallery.status = Some(e);
            } else {
                self.cad.gallery.open = false;
            }
        }
        eframe::App::logic(&mut self.cad, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.cad, ui, frame);
        let ctx = ui.ctx().clone();
        if self.cad.touch.fullscreen != self.fullscreen_applied {
            self.fullscreen_applied = self.cad.touch.fullscreen;
            bridge::set_fullscreen(&self.android, self.fullscreen_applied);
        }
        if std::mem::take(&mut self.cad.touch.keyboard_request) {
            self.cad.touch.block_keyboard = false;
            bridge::force_show_soft_keyboard(&self.android);
        }
        if !self.loading_hidden {
            self.loading_hidden = true;
            bridge::set_loading_progress(&self.android, 100);
            bridge::hide_loading(&self.android);
        }
        let copied = ctx.output(|o| {
            o.commands.iter().rev().find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
        });
        if let Some(text) = copied.filter(|t| *t != self.last_copied) {
            bridge::clipboard_set(&self.android, &text);
            self.last_copied = text;
        }
    }

    fn on_exit(&mut self) {
        eframe::App::on_exit(&mut self.cad);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}
