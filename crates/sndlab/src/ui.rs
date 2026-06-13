//! egui rendering: top toolbar, status bar, log pane, scope, editor.
//!
//! The layout reserves about a third of the central area for the
//! scope (right side), with the editor filling the rest. Log and
//! status are stacked at the bottom.

use egui::{Color32, RichText};
use egui_code_editor::CodeEditor;

use crate::app::SndlabApp;
use crate::log::LogKind;
use crate::scope;

pub fn draw(ui: &mut egui::Ui, app: &mut SndlabApp) {
    // Hotkeys check first so they fire regardless of which panel
    // currently has focus.
    if ui.input(|i| i.key_pressed(egui::Key::F5)) {
        app.eval_and_play();
    }

    egui::Panel::top("toolbar").show_inside(ui, |ui| toolbar(ui, app));
    egui::Panel::bottom("status").show_inside(ui, |ui| status(ui, app));
    egui::Panel::bottom("log")
        .resizable(true)
        .default_size(140.0)
        .show_inside(ui, |ui| log_pane(ui, app));
    egui::Panel::right("scope")
        .resizable(true)
        .default_size(360.0)
        .show_inside(ui, |ui| scope_pane(ui, app));
    egui::CentralPanel::default().show_inside(ui, |ui| editor(ui, app));
}

fn toolbar(ui: &mut egui::Ui, app: &mut SndlabApp) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("sndlab").strong());
        ui.separator();
        if ui
            .button(RichText::new("Eval + Play (F5)").color(Color32::LIGHT_GREEN))
            .clicked()
        {
            app.eval_and_play();
        }
        ui.separator();
        // Patch picker: lets the user trigger any registered patch by
        // name, not just the first one. Hidden until at least one
        // patch is registered so the toolbar isn't empty at startup.
        let patches: Vec<_> = app
            .engine
            .patches()
            .iter()
            .map(|p| (p.name.clone(), p.duration_s))
            .collect();
        if !patches.is_empty() {
            ui.label("Play:");
            for (name, dur) in patches {
                if ui
                    .small_button(format!("{} ({:.1}s)", name, dur))
                    .clicked()
                {
                    app.play_by_name(&name);
                }
            }
        }
    });
}

fn status(ui: &mut egui::Ui, app: &SndlabApp) {
    ui.horizontal(|ui| {
        let mcp = if app.mcp_endpoint.is_empty() {
            "MCP: -".to_string()
        } else {
            format!("MCP: {}", app.mcp_endpoint)
        };
        ui.monospace(format!("file: {}", app.filename));
        ui.separator();
        ui.monospace(format!(
            "buffer: {} chars",
            app.code.len()
        ));
        ui.separator();
        ui.monospace(format!(
            "audio: {}",
            if app.engine.has_audio() { "on" } else { "off" }
        ));
        ui.separator();
        ui.monospace(mcp);
    });
}

fn log_pane(ui: &mut egui::Ui, app: &SndlabApp) {
    ui.label(RichText::new("log").color(Color32::GRAY).small());
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in app.log.entries() {
                let (tag, colour) = match entry.kind {
                    LogKind::Info => ("INFO ", Color32::GRAY),
                    LogKind::Warn => ("WARN ", Color32::YELLOW),
                    LogKind::Error => ("ERROR", Color32::LIGHT_RED),
                    LogKind::Audio => ("AUDIO", Color32::LIGHT_BLUE),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tag).color(colour).strong().monospace());
                    ui.monospace(&entry.line);
                });
            }
        });
}

fn scope_pane(ui: &mut egui::Ui, app: &SndlabApp) {
    ui.label(RichText::new("scope").color(Color32::GRAY).small());
    scope::show(
        ui,
        app.last_buffer.as_ref(),
        app.last_spectrum.as_deref(),
    );
}

fn editor(ui: &mut egui::Ui, app: &mut SndlabApp) {
    CodeEditor::default()
        .id_source("sndlab.editor")
        .with_rows(24)
        .with_fontsize(14.0)
        .with_theme(app.theme)
        .with_numlines(true)
        .vscroll(true)
        .show(ui, &mut app.code, &app.syntax);
}
