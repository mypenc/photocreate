//! The iOS host: the PhotoCraft engine and egui UI in the compact layout.
//! Artworks live in `Documents/Artworks` (visible in the Files app).

use photocraft_engine::Session;
use photocraft_ui_egui::PhotocraftApp;

use crate::mobile_services;

struct IosHost {
    cad: PhotocraftApp,
}

impl eframe::App for IosHost {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.cad, ctx, raw);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.cad, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.cad, ui, frame);
    }

    fn on_exit(&mut self) {
        eframe::App::on_exit(&mut self.cad);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

pub fn run() -> eframe::Result {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC on thread {:?}: {info}", std::thread::current().name());
    }));
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_title("PhotoCreate"), stencil_buffer: 8, ..Default::default() };
    eframe::run_native(
        "PhotoCreate",
        options,
        Box::new(move |cc| {
            PhotocraftApp::install_type_reset_hooks(&cc.egui_ctx);
            let mut cad = PhotocraftApp::new(Session::new(), mobile_services::mobile());
            cad.touch.enabled = true;
            cad.background_jobs = false;
            if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
                let root = std::path::PathBuf::from(home).join("Documents").join("Artworks");
                let _ = std::fs::create_dir_all(&root);
                cad.gallery.root = Some(root);
            }
            cad.gallery.open = true;
            Ok(Box::new(IosHost { cad }))
        }),
    )
}
