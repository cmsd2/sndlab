//! sndlab: a graphical sound-design environment.
//!
//! eframe window + egui UI + egui_code_editor for the patch editor +
//! a custom scope widget for the rendered waveform.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod log;
mod mcp;
mod project;
mod reference;
mod scope;
mod spectrum;
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
        // Drain MCP commands and republish state before drawing so
        // any AI edits that arrived this frame land in the editor
        // pane the user is about to see.
        self.pump_mailbox();
        ui::draw(ui, self);
        // Keep the loop ticking even without user input so MCP-
        // triggered edits/plays don't sit invisible until the user
        // moves the mouse. 30 fps is plenty for our needs.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
}
