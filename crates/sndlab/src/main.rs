//! sndlab: a graphical sound-design environment.
//!
//! eframe window + egui UI + egui_code_editor for the patch editor +
//! a custom scope widget for the rendered waveform.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod log;
mod scope;
mod ui;

use crate::app::SndlabApp;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("sndlab"),
        ..Default::default()
    };

    eframe::run_native(
        "sndlab",
        options,
        Box::new(|_cc| Ok(Box::new(SndlabApp::new()))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}

impl eframe::App for SndlabApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::draw(ui, self);
    }
}
