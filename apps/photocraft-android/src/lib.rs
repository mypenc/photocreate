//! CADCraft for Android.
//!
//! The same engine and egui interface as the desktop app (`apps/cadcraft`), compiled as a
//! `cdylib` that a GameActivity (see `android/`) loads. There is no web view: eframe draws with
//! wgpu (Vulkan, OpenGL ES fallback) straight into the activity's surface.
//!
//! What differs from the desktop app:
//!
//! * **Layout.** Phones and pads get the compact layout (`cadcraft_ui_egui::compact`): a menu
//!   button, a touch-sized tool bar, a fixed bottom dock of drafting toggles, and Layers /
//!   Properties as a side panel or sheet. The floating touch bar is off by default (View ▸ Touch).
//! * **Files.** Android hands apps `content://` URIs, not paths. The pickers go to Kotlin
//!   ([`bridge`]): Open copies the drawing into the app's folder; Save writes there and
//!   `Exports.kt` mirrors it to the file the user named. The pickers never block this thread
//!   (see [`bridge::pick`]): the command that asked is run again when the user has chosen.
//! * **Input.** Fingers become mouse events in `cadcraft_ui_egui::touch` (long press = right
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
// The desktop app's logger (standard error → logcat, plus a log file); it only needs `log`.
#[allow(dead_code)]
#[path = "../../cadcraft/src/logging.rs"]
mod logging;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use android_activity::AndroidApp;
use android_activity::input::{Axis, ImeOptions, InputType, TextInputAction};
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

/// Stencil bits of the window's render pass (see `NativeOptions::stencil_buffer`).
const STENCIL_BITS: u8 = 8;

const PREFS_EVERY: Duration = Duration::from_secs(3);

/// Entry point: android-activity calls this on its own thread, not the Java UI thread.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    prepare_environment(&app);
    bridge::set_loading_progress(&app, 10);
    let logger = logging::install();
    install_panic_hook();
    if let Some(logger) = logger {
        match config_dir() {
            Some(dir) => match logger.attach_dir(&dir.join(logging::LOG_DIR)) {
                Ok(path) => log::info!("CADCraft {} for Android, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
                Err(e) => log::warn!("no log file: {e}"),
            },
            None => logger.stderr_only(),
        }
    }
    // Mouse wheel axes are off by default in GameActivity.
    app.enable_motion_axis(Axis::Hscroll);
    app.enable_motion_axis(Axis::Vscroll);
    bridge::set_loading_progress(&app, 20);
    install_io();
    bridge::set_loading_progress(&app, 35);

    let options = eframe::NativeOptions {
        android_app: Some(app.clone()),
        wgpu_options: wgpu_options(),
        // A stencil buffer clips the contents of clipped layout viewports on the GPU.
        stencil_buffer: STENCIL_BITS,
        ..Default::default()
    };
    let host = app.clone();
    let result = eframe::run_native(
        "CADCraft",
        options,
        Box::new(move |cc| {
            match &cc.wgpu_render_state {
                Some(rs) => log::info!("graphics: {:?}", rs.adapter.get_info()),
                None => log::info!("graphics: no wgpu device"),
            }
            bridge::set_loading_progress(&host, 55);
            bridge::set_context(cc.egui_ctx.clone());
            let mut cad = CadApp::new(Session::empty(), services(&host));
            bridge::set_loading_progress(&host, 65);
            cad.touch.enabled = true;
            if let Some(tag) = bridge::locale_tag(&host) {
                cad.system_languages = vec![tag];
            }
            load_settings(&mut cad);
            bridge::set_loading_progress(&host, 80);
            // Autosave files go to `<settings>/autosave`; a crash leaves them for the recovery prompt.
            if let Some(dir) = config_dir() {
                cad.enable_autosave(&dir.join("autosave"));
                let support = dir.join("support");
                if let Err(e) = std::fs::create_dir_all(&support) {
                    log::warn!("no LISP support folder {}: {e}", support.display());
                }
                cad.session.set_lisp_support_dir(Some(support));
            }
            if let Some(rs) = &cc.wgpu_render_state {
                cad.set_wgpu(rs, eframe::egui_wgpu::depth_format_from_bits(0, STENCIL_BITS));
            }
            if cad.session.docs.is_empty() {
                cad.session.new_drawing(false);
            }
            bridge::set_loading_progress(&host, 95);
            // Multi-line text: Gboard shows Enter, not the Done check. No-suggestions keeps Gboard from
            // composing words (it re-sent each word, many Backspaces per keystroke).
            host.set_ime_editor_info(
                InputType::from_bits_retain(0x1 | 0x4000 | 0x2_0000 | 0x8_0000),
                TextInputAction::None,
                ImeOptions::from_bits_retain(0x4000_0000 | 0x1000_0000),
            );
            let h = host.clone();
            cad.touch.haptic = Some(Box::new(move || bridge::haptic(&h)));
            Ok(Box::new(AndroidHost {
                cad,
                android: host,
                last_copied: String::new(),
                fullscreen_applied: false,
                block_applied: false,
                loading_hidden: false,
                folder_poll: 0,
                last_prefs_check: Instant::now(),
                saved: Vec::new(),
            }))
        }),
    );
    if let Err(e) = result {
        log::error!("CADCraft could not start: {e}");
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
        std::env::set_var("CADCRAFT_CONFIG_DIR", &config);
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}

/// The settings folder (`<files>/config`): logs, autosave, LISP support, preferences.
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CADCRAFT_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// File format hooks for the engine (it stays I/O-agnostic). Same as the desktop app's.
fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
        groups: cadcraft_io::entity_groups,
        entity: cadcraft_io::entity_from_groups,
    });
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
                label: Some("CADCraft device"),
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

/// The system pickers as the desktop UI's `Services`. They never block: see [`bridge::pick`].
fn services(host: &AndroidApp) -> Services {
    let open = host.clone();
    let save = host.clone();
    Services {
        pick_open: Some(Box::new(move || bridge::pick(&open, bridge::PickKind::Open, ""))),
        pick_save: Some(Box::new(move |name: &str| bridge::pick(&save, bridge::PickKind::Save, name))),
    }
}

// ---- settings --------------------------------------------------------------------------------
// Three small JSON files in the settings folder (the desktop keeps the same three in eframe's
// storage): the drafting profile, the interface preferences and the autosave preferences.

fn settings_file(name: &str) -> Option<PathBuf> {
    config_dir().map(|d| d.join(name))
}

fn read_setting(name: &str) -> Option<String> {
    std::fs::read_to_string(settings_file(name)?).ok()
}

fn load_settings(cad: &mut CadApp) {
    if let Some(profile) = read_setting("profile.json") {
        cad.load_profile(&profile);
    }
    if let Some(prefs) = read_setting("prefs.json") {
        cad.load_prefs(&prefs);
    }
    if let Some(prefs) = read_setting("autosave.json") {
        cadcraft_ui_egui::autosave::load_prefs(&prefs);
    }
}

/// The three settings texts as they are now.
fn settings_snapshot(cad: &CadApp) -> Vec<(&'static str, String)> {
    vec![
        ("profile.json", cad.profile_json()),
        ("prefs.json", cad.prefs_json()),
        ("autosave.json", cadcraft_ui_egui::autosave::prefs_json()),
    ]
}

/// Write beside the file and rename, so a crash mid-write never leaves half a settings file.
fn write_setting(name: &str, text: &str) -> bool {
    let Some(path) = settings_file(name) else { return false };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &path).is_ok()
}

// ---- the eframe app ----------------------------------------------------------------------------

struct AndroidHost {
    cad: CadApp,
    android: AndroidApp,
    /// The last text mirrored to the system clipboard (so an unchanged copy isn't sent twice).
    last_copied: String,
    /// Whether View ▸ Touch ▸ Full Screen is what Android was last told.
    fullscreen_applied: bool,
    /// Whether Block On-screen Keyboard is what Android was last told.
    block_applied: bool,
    /// The startup loading bar was removed.
    loading_hidden: bool,
    /// Frames until the documents folder state is read from Android again.
    folder_poll: u32,
    /// When preferences were last checked for changes.
    last_prefs_check: Instant,
    /// The settings texts last written.
    saved: Vec<String>,
}

impl AndroidHost {
    /// Settings are written soon after they change, not only on a clean exit, so a crash or a
    /// killed app keeps them.
    fn save_settings_if_changed(&mut self, force: bool) {
        if !force && self.last_prefs_check.elapsed() < PREFS_EVERY {
            return;
        }
        self.last_prefs_check = Instant::now();
        let now = settings_snapshot(&self.cad);
        let texts: Vec<String> = now.iter().map(|(_, t)| t.clone()).collect();
        if texts == self.saved {
            return;
        }
        let mut all_written = true;
        for (name, text) in &now {
            all_written &= write_setting(name, text);
        }
        if all_written {
            self.saved = texts;
        }
    }
}

impl eframe::App for AndroidHost {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.cad.touch.process(ctx, raw, None, &[]);
        self.cad.raw_input_hook(raw);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for path in bridge::take_opened() {
            self.cad.open_path(&path);
        }
        // The user chose a file in the system picker: run the command that asked again, now it has one.
        if let Some(invocation) = bridge::take_replay() {
            self.cad.replay(invocation);
        }
        if bridge::take_tap_again() {
            self.cad.set_status("File chosen. Tap the button again to use it.");
        }
        self.cad.logic(ctx);
        self.save_settings_if_changed(false);
        // The window's back/close request: with unsaved changes the UI asks first.
        if std::mem::take(&mut self.cad.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);
        let ctx = ui.ctx().clone();
        self.cad.touch_frame(&ctx);
        // View ▸ Touch ▸ Full Screen and Documents Folder.
        if self.cad.touch.fullscreen != self.fullscreen_applied {
            self.fullscreen_applied = self.cad.touch.fullscreen;
            bridge::set_fullscreen(&self.android, self.fullscreen_applied);
        }
        if let Some(on) = self.cad.touch.docs_folder_request.take() {
            bridge::set_documents_folder(&self.android, on);
            self.folder_poll = 0;
        }
        // The folder picker answers later, so ask Android now and then what the state is.
        if self.folder_poll == 0 {
            if let Some(set) = bridge::documents_folder_set(&self.android) {
                self.cad.touch.docs_folder = set;
            }
            self.folder_poll = 120;
        }
        self.folder_poll -= 1;
        // Block keyboard: the keyboard is never asked for (`Touch::filter_ime` drops the request),
        // so there is nothing to close each frame. Closing happens once, when the switch is turned
        // on while Gboard is already up. The bar's KB button opens it by force.
        let force_keyboard = std::mem::take(&mut self.cad.touch.keyboard_request);
        if force_keyboard {
            self.cad.touch.block_keyboard = false;
            self.block_applied = false;
            bridge::force_show_soft_keyboard(&self.android);
        }
        if self.cad.touch.block_keyboard != self.block_applied {
            self.block_applied = self.cad.touch.block_keyboard;
            if self.block_applied && bridge::soft_keyboard_visible(&self.android) == Some(true) {
                bridge::hide_soft_keyboard(&self.android);
            }
        }
        // The first frame is drawn: take the system loading bar away. This is also what tells the
        // crash gate (`DebugLog.markReady`) that this launch got going.
        if !self.loading_hidden {
            self.loading_hidden = true;
            bridge::set_loading_progress(&self.android, 100);
            bridge::hide_loading(&self.android);
        }
        // Copy / Cut → the system clipboard.
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
        // A normal exit (after any "Save changes?" answer) leaves no autosave behind.
        self.cad.shutdown_autosave();
        self.save_settings_if_changed(true);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}
