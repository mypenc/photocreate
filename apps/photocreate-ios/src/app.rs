//! The iOS host: settings in `Documents/config`, the same hooks as the desktop app.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

use crate::logging;

const STENCIL_BITS: u8 = 8;
const PREFS_EVERY: Duration = Duration::from_secs(3);

/// `Documents/config`: logs, autosave, LISP support, preferences. Files.app shows `Documents`.
fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty()).map(PathBuf::from)?;
    let dir = home.join("Documents").join("config");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

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

fn read_setting(name: &str) -> Option<String> {
    std::fs::read_to_string(config_dir()?.join(name)).ok()
}

fn write_setting(name: &str, text: &str) -> bool {
    let Some(path) = config_dir().map(|d| d.join(name)) else { return false };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &path).is_ok()
}

struct IosHost {
    cad: CadApp,
    last_check: Instant,
    saved: Vec<String>,
}

impl IosHost {
    fn save_if_changed(&mut self, force: bool) {
        if !force && self.last_check.elapsed() < PREFS_EVERY {
            return;
        }
        self.last_check = Instant::now();
        let now = [
            ("profile.json", self.cad.profile_json()),
            ("prefs.json", self.cad.prefs_json()),
            ("autosave.json", cadcraft_ui_egui::autosave::prefs_json()),
        ];
        let texts: Vec<String> = now.iter().map(|(_, t)| t.clone()).collect();
        if texts == self.saved {
            return;
        }
        let mut ok = true;
        for (name, text) in &now {
            ok &= write_setting(name, text);
        }
        if ok {
            self.saved = texts;
        }
    }
}

impl eframe::App for IosHost {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.cad.touch.process(ctx, raw, None, &[]);
        self.cad.raw_input_hook(raw);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.cad.logic(ctx);
        self.save_if_changed(false);
        if std::mem::take(&mut self.cad.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);
        let ctx = ui.ctx().clone();
        self.cad.touch_frame(&ctx);
    }

    fn on_exit(&mut self) {
        self.cad.shutdown_autosave();
        self.save_if_changed(true);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

pub fn run() -> eframe::Result {
    let logger = logging::install();
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC on thread {:?}: {info}\n{backtrace}", std::thread::current().name());
    }));
    if let Some(logger) = logger {
        match config_dir().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("CADCraft {} for iOS, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => logger.stderr_only(),
        }
    }
    install_io();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("CADCraft"),
        stencil_buffer: STENCIL_BITS,
        ..Default::default()
    };
    eframe::run_native(
        "CADCraft",
        options,
        Box::new(move |cc| {
            let mut cad = CadApp::new(Session::empty(), Services { pick_open: None, pick_save: None });
            cad.touch.enabled = true;
            if let Some(profile) = read_setting("profile.json") {
                cad.load_profile(&profile);
            }
            if let Some(prefs) = read_setting("prefs.json") {
                cad.load_prefs(&prefs);
            }
            if let Some(prefs) = read_setting("autosave.json") {
                cadcraft_ui_egui::autosave::load_prefs(&prefs);
            }
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
            Ok(Box::new(IosHost { cad, last_check: Instant::now(), saved: Vec::new() }))
        }),
    )
}
