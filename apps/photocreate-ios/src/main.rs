//! PhotoCreate for iPhone and iPad.
//!
//! The same engine and egui interface as the desktop app, drawn by wgpu (Metal) into a winit
//! window. Phones and pads get the compact layout (`photocraft_ui_egui::compact`); fingers become
//! mouse events through `photocraft_ui_egui::touch`. Settings, autosave and logs live in the app's
//! Documents folder (visible in the Files app: "PhotoCreate" with `UIFileSharingEnabled`).
//!
//! Not yet on iOS: the system document picker (File ▸ Open / Save As use paths typed on the
//! command line or the Files-shared Documents folder), the control server, SpaceMouse, print.
//!
//! On other targets this is an empty program, so the workspace builds everywhere.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("photocreate-ios only runs on iOS; build it with --target aarch64-apple-ios");
}

#[cfg(target_os = "ios")]
mod app;

#[cfg(target_os = "ios")]
mod mobile_services;

#[cfg(target_os = "ios")]
fn main() -> eframe::Result {
    app::run()
}
