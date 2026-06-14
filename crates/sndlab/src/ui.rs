//! egui rendering: top toolbar, status bar, log pane, scope, editor.
//!
//! The layout reserves about a third of the central area for the
//! scope (right side), with the editor filling the rest. Log and
//! status are stacked at the bottom.

use egui::{Color32, RichText};
use egui_code_editor::CodeEditor;

use sndlab_core::PatchRole;

use crate::app::{Modal, PendingAction, SndlabApp};
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

    // Modal — rendered last so it draws on top of the panels.
    render_modal(ui.ctx(), app);
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
        if ui.small_button("New").clicked() {
            app.new_project();
        }
        if ui.small_button("Open...").clicked() {
            app.open_project();
        }
        if ui.small_button("Save").clicked() {
            app.save_project();
        }
        if ui.small_button("Save As...").clicked() {
            app.save_project_as();
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
        // Split patches by role so the controls match what each
        // does: ambient = toggle on/off (lit when looping), one-shot
        // = trigger + arm.
        let patches: Vec<(String, PatchRole, f32)> = app
            .engine
            .patches()
            .iter()
            .map(|p| (p.name.clone(), p.role, p.duration_s))
            .collect();
        let mut ambient_present = false;
        let mut one_shot_present = false;
        for (name, role, dur) in &patches {
            match role {
                PatchRole::Ambient => {
                    if !ambient_present {
                        ui.label("Ambient:");
                        ui.checkbox(&mut app.live_ambient, "Live")
                            .on_hover_text(
                                "Crossfade currently-playing ambients into the new \
                                 buffer on every eval (live-coding mode).",
                            );
                        ambient_present = true;
                    }
                    let playing = app.engine.is_ambient_playing(name);
                    let label = if playing {
                        RichText::new(format!("● {}", name)).color(Color32::LIGHT_GREEN)
                    } else {
                        RichText::new(format!("○ {}", name))
                    };
                    if ui.small_button(label).clicked() {
                        app.toggle_ambient(name);
                    }
                }
                PatchRole::OneShot => {
                    if !one_shot_present {
                        if ambient_present {
                            ui.separator();
                        }
                        ui.label("One-shot:");
                        one_shot_present = true;
                    }
                    if ui
                        .small_button(format!("{} ({:.1}s)", name, dur))
                        .clicked()
                    {
                        app.play_by_name(name);
                    }
                    let mut armed = app.armed.contains(name);
                    if ui.checkbox(&mut armed, "arm").changed() {
                        app.toggle_arm(name);
                    }
                }
            }
        }
        if !app.armed.is_empty() {
            ui.separator();
            if ui
                .button(
                    RichText::new(format!("Fire scene ({})", app.armed.len()))
                        .color(Color32::LIGHT_YELLOW),
                )
                .clicked()
            {
                app.fire_scene();
            }
        }
        ui.separator();
        if ui.small_button("Load reference...").clicked() {
            app.pick_and_load_reference();
        }
        if let Some(name) = &app.reference_name {
            ui.colored_label(
                Color32::from_rgb(240, 180, 100),
                format!("ref: {name}"),
            );
            if ui.small_button("clear").clicked() {
                app.clear_reference();
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
        let dirty_marker = if app.project.is_dirty() { " *" } else { "" };
        let where_ = match app.project.root.as_ref() {
            Some(p) => p.display().to_string(),
            None => "(unsaved)".to_string(),
        };
        ui.monospace(format!(
            "project: {}{} @ {}",
            app.project.manifest.name, dirty_marker, where_
        ));
        ui.separator();
        ui.monospace(format!("script: {}", app.project.scripts[app.project.active].relative_path));
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
        app.reference_buffer.as_ref(),
        app.reference_spectrum.as_deref(),
    );
}

/// Fixed-height status / error banner that sits above the editor.
/// Always present so the editor's vertical position doesn't shift
/// when an error appears or clears. Three lines of monospace text:
/// the header (filename + line:col + message), the offending source
/// line, and a caret line pointing at the column.
fn render_status_banner(ui: &mut egui::Ui, app: &SndlabApp) {
    const ROWS: usize = 3;
    const FONT_SIZE: f32 = 12.0;
    // Approximate row height; egui's FontsView doesn't expose a
    // public per-font query here, but for the bundled monospace
    // 1.4× the point size matches the actual line height to within
    // a pixel.
    let row_h = FONT_SIZE * 1.4;
    let font_id = egui::FontId::monospace(FONT_SIZE);
    let inner_pad = 4.0;
    let total_h = row_h * ROWS as f32 + inner_pad * 2.0;

    let (rect, _) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), total_h),
        egui::Sense::hover(),
    );

    let (bg, border, fg, muted) = match app.live.error.is_some() {
        true => (
            Color32::from_rgb(56, 16, 16),
            Color32::from_rgb(120, 40, 40),
            Color32::from_rgb(255, 200, 190),
            Color32::from_rgb(200, 140, 130),
        ),
        false => (
            Color32::from_rgb(16, 28, 22),
            Color32::from_rgb(30, 50, 40),
            Color32::from_rgb(160, 200, 180),
            Color32::from_rgb(120, 160, 140),
        ),
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, bg);
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );

    let x0 = rect.left() + 8.0;
    let mut y = rect.top() + inner_pad;

    let header = match &app.live.error {
        Some(err) => match &err.context {
            Some(ctx) => format!(
                "✕ {}:{}:{}  {}",
                ctx.filename, ctx.line, ctx.column, err.message
            ),
            None => format!("✕ {}", err.message),
        },
        None => {
            if app.live_ambient {
                "✓ live — script ok, ambients will crossfade on edit".to_string()
            } else {
                "✓ script ok".to_string()
            }
        }
    };
    painter.text(
        egui::Pos2::new(x0, y),
        egui::Align2::LEFT_TOP,
        header,
        font_id.clone(),
        fg,
    );
    y += row_h;

    // If we have full context, show the source line and a caret.
    if let Some(err) = &app.live.error {
        if let Some(ctx) = &err.context {
            painter.text(
                egui::Pos2::new(x0, y),
                egui::Align2::LEFT_TOP,
                &ctx.source_line,
                font_id.clone(),
                muted,
            );
            y += row_h;

            // Approximate per-character advance for a monospace font.
            // egui doesn't have a direct "glyph width" API at this
            // call site, but `row_height * 0.55` is a near-universal
            // ratio for the bundled monospace and works well for
            // simple ASCII source.
            let glyph_w = row_h * 0.55;
            let caret_col = ctx.column.saturating_sub(1) as f32;
            let caret = "^";
            painter.text(
                egui::Pos2::new(x0 + caret_col * glyph_w, y),
                egui::Align2::LEFT_TOP,
                caret,
                font_id,
                fg,
            );
        }
    }
}

fn render_modal(ctx: &egui::Context, app: &mut SndlabApp) {
    let Some(modal) = app.modal.clone() else {
        return;
    };
    // Track a pending close so we can swap modal state cleanly
    // outside the closures.
    let mut close_modal = false;
    let mut new_modal_state: Option<Modal> = None;

    match modal {
        Modal::ConfirmDiscard { action } => {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "The current project has unsaved changes. {} anyway?",
                        match action {
                            PendingAction::NewProject => "Discard and start a new project",
                            PendingAction::OpenProject => "Discard and open a different project",
                        }
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Discard").color(Color32::LIGHT_RED))
                            .clicked()
                        {
                            close_modal = true;
                            app.discard_and(action);
                        }
                        if ui.button("Cancel").clicked() {
                            close_modal = true;
                        }
                    });
                });
        }
        Modal::EditFilename { index, mut input } => {
            let title = if index.is_some() {
                "Rename script"
            } else {
                "Add script"
            };
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Filename (e.g. ambience.rhai):");
                    let resp = ui.text_edit_singleline(&mut input);
                    // Keep the input live in the modal state so the
                    // user sees what they're typing.
                    new_modal_state = Some(Modal::EditFilename {
                        index,
                        input: input.clone(),
                    });
                    let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() || submit {
                            close_modal = true;
                            match index {
                                Some(i) => app.rename_script(i, input.clone()),
                                None => app.add_script(input.clone()),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_modal = true;
                        }
                    });
                });
        }
        Modal::ConfirmDelete { index } => {
            let name = app
                .project
                .scripts
                .get(index)
                .map(|s| s.relative_path.clone())
                .unwrap_or_default();
            egui::Window::new("Delete script")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Delete {name} from the project? This also removes the file from disk."
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Delete").color(Color32::LIGHT_RED))
                            .clicked()
                        {
                            close_modal = true;
                            app.delete_script(index);
                        }
                        if ui.button("Cancel").clicked() {
                            close_modal = true;
                        }
                    });
                });
        }
    }

    if close_modal {
        app.modal = None;
    } else if let Some(new) = new_modal_state {
        // Persist the user's in-progress typing.
        app.modal = Some(new);
    }
}

/// Generate a filename like `untitled.rhai` or `untitled-2.rhai`
/// that doesn't collide with an existing script.
fn default_new_filename(app: &SndlabApp) -> String {
    let exists = |name: &str| {
        app.project
            .scripts
            .iter()
            .any(|s| s.relative_path == name)
    };
    if !exists("untitled.rhai") {
        return "untitled.rhai".into();
    }
    for n in 2..1000 {
        let candidate = format!("untitled-{n}.rhai");
        if !exists(&candidate) {
            return candidate;
        }
    }
    "untitled-many.rhai".into()
}

fn editor(ui: &mut egui::Ui, app: &mut SndlabApp) {
    // Live-eval status banner. Always rendered with the same
    // vertical footprint so the editor doesn't pop up/down as
    // errors come and go — sized to fit three monospace lines plus
    // padding (header, source line, caret), which is the worst-case
    // content.
    render_status_banner(ui, app);

    // Tabs across the top: one per script. Click to switch active.
    // Right-click for Rename / Delete. A "+" tab at the end creates
    // a new script.
    ui.horizontal_wrapped(|ui| {
        let scripts_len = app.project.scripts.len();
        let mut new_active: Option<usize> = None;
        let mut rename_request: Option<(usize, String)> = None;
        let mut delete_request: Option<usize> = None;
        for i in 0..scripts_len {
            let is_active = i == app.project.active;
            let script = &app.project.scripts[i];
            let label = if script.dirty {
                format!("{} *", script.relative_path)
            } else {
                script.relative_path.clone()
            };
            let resp = ui.selectable_label(is_active, label);
            if resp.clicked() {
                new_active = Some(i);
            }
            resp.context_menu(|ui| {
                if ui.button("Rename...").clicked() {
                    rename_request = Some((i, app.project.scripts[i].relative_path.clone()));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    delete_request = Some(i);
                    ui.close();
                }
            });
        }
        if ui.small_button("+").on_hover_text("Add script").clicked() {
            app.modal = Some(Modal::EditFilename {
                index: None,
                input: default_new_filename(app),
            });
        }
        if let Some(i) = new_active {
            app.project.active = i;
        }
        if let Some((i, current)) = rename_request {
            app.modal = Some(Modal::EditFilename {
                index: Some(i),
                input: current,
            });
        }
        if let Some(i) = delete_request {
            app.modal = Some(Modal::ConfirmDelete { index: i });
        }
    });
    ui.separator();

    let theme = app.theme;
    let syntax = app.syntax.clone();
    // Snapshot before render so we can detect whether the user
    // actually typed this frame (vs the editor merely repainting).
    // Marking dirty only on real change keeps the unsaved-marker
    // honest.
    let before_len = app.project.active_buffer().len();
    let buffer = app.project.active_buffer_mut();
    let buffer_addr_before: *const u8 = buffer.as_ptr();
    CodeEditor::default()
        .id_source("sndlab.editor")
        .with_rows(24)
        .with_fontsize(14.0)
        .with_theme(theme)
        .with_numlines(true)
        .vscroll(true)
        .show(ui, buffer, &syntax);
    // Two cheap heuristics for "buffer changed": length differs, or
    // the storage pointer moved (egui resized String capacity). Both
    // are easier than a full content compare and catch every typed
    // character. False positives are rare and harmless — the worst
    // case is a buffer that was edited and then restored looks dirty.
    let after_len = app.project.active_buffer().len();
    let buffer_addr_after = app.project.active_buffer().as_ptr();
    if before_len != after_len || buffer_addr_before != buffer_addr_after {
        app.project.mark_active_dirty();
        app.note_typed();
    }
}
