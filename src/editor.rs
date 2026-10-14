//! Minimal dark editor: controls + live preview, same renderer as overlay.

use std::process::Child;

use eframe::egui::{CentralPanel, Color32, ComboBox, DragValue, Frame, Grid, Panel};
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::config::{
    self, MAX_GAP, MAX_OFFSET, MAX_OPACITY, MAX_SIZE, MAX_THICKNESS, MIN_GAP, MIN_OPACITY,
    MIN_SIZE, MIN_THICKNESS, Settings,
};
use crate::geometry::Shape;
use crate::platform;
use crate::render::paint_crosshair;

pub struct EditorApp {
    settings: Settings,
    status: String,
    color_text: String,
    /// The overlay process started from here, if any. Killed when the editor
    /// closes or restarts the overlay, so closing the app closes the overlay.
    overlay: Option<Child>,
}

impl EditorApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(eframe::egui::Visuals::dark());
        let (settings, path, err) = config::load();
        let status = match err {
            Some(err) => format!("config {path:?}: {err}; defaults loaded"),
            None => format!("loaded {path:?}"),
        };
        let color_text = hex_of(settings.color);
        Self {
            settings,
            status,
            color_text,
            overlay: None,
        }
    }

    fn save(&mut self) {
        self.settings.clamp();
        match config::save(&self.settings) {
            Ok(path) => self.status = format!("saved {path:?}"),
            Err(err) => self.status = format!("save failed: {err}"),
        }
    }

    /// Start an overlay, replacing any overlay this editor started before.
    fn restart_overlay(&mut self) {
        self.kill_overlay();
        match launch_overlay_child() {
            Ok(child) => {
                self.overlay = Some(child);
                self.status = "overlay launched".to_owned();
            }
            Err(err) => self.status = err,
        }
    }

    /// Stop the overlay process this editor started, if it is still running.
    fn kill_overlay(&mut self) {
        if let Some(mut child) = self.overlay.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn launch_overlay_child() -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|err| format!("cannot locate own exe: {err}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--overlay");
    platform::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|err| format!("launch overlay failed: {err}"))
}

fn hex_of(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Cross => "cross",
        Shape::Dot => "dot",
        Shape::Circle => "circle",
        Shape::T => "t",
    }
}

impl App for EditorApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut EframeFrame) {
        Panel::left("controls").resizable(false).show(ui, |ui| {
            ui.heading("vibehair");
            ui.separator();
            Grid::new("settings")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("shape");
                    ComboBox::from_id_salt("shape")
                        .selected_text(shape_name(self.settings.shape))
                        .show_ui(ui, |ui| {
                            for shape in [Shape::Cross, Shape::Dot, Shape::Circle, Shape::T] {
                                ui.selectable_value(
                                    &mut self.settings.shape,
                                    shape,
                                    shape_name(shape),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label("size");
                    ui.add(
                        DragValue::new(&mut self.settings.size)
                            .range(MIN_SIZE..=MAX_SIZE)
                            .speed(0.5),
                    );
                    ui.end_row();
                    ui.label("gap");
                    ui.add(
                        DragValue::new(&mut self.settings.gap)
                            .range(MIN_GAP..=MAX_GAP)
                            .speed(0.5),
                    );
                    ui.end_row();
                    ui.label("thickness");
                    ui.add(
                        DragValue::new(&mut self.settings.thickness)
                            .range(MIN_THICKNESS..=MAX_THICKNESS)
                            .speed(0.1),
                    );
                    ui.end_row();
                    ui.label("opacity");
                    ui.add(
                        DragValue::new(&mut self.settings.opacity)
                            .range(MIN_OPACITY..=MAX_OPACITY)
                            .speed(0.01),
                    );
                    ui.end_row();
                    ui.label("color");
                    if ui.text_edit_singleline(&mut self.color_text).changed()
                        && let Some(color) = config::parse_hex_color(self.color_text.trim())
                    {
                        self.settings.color = color;
                    }
                    ui.end_row();
                    ui.label("offset x");
                    ui.add(
                        DragValue::new(&mut self.settings.offset_x).range(-MAX_OFFSET..=MAX_OFFSET),
                    );
                    ui.end_row();
                    ui.label("offset y");
                    ui.add(
                        DragValue::new(&mut self.settings.offset_y).range(-MAX_OFFSET..=MAX_OFFSET),
                    );
                    ui.end_row();
                });
            ui.separator();
            if ui.button("save").clicked() {
                self.save();
            }
            if ui.button("launch overlay").clicked() {
                self.save();
                self.restart_overlay();
            }
            ui.separator();
            ui.small(&self.status);
        });
        CentralPanel::default().show(ui, |ui| {
            ui.heading("preview");
            Frame::canvas(ui.style()).show(ui, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(ui.available_size(), eframe::egui::Sense::hover());
                ui.set_clip_rect(rect);
                // Checkerboard-ish dark backdrop via two-tone fill for visibility.
                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(24));
                paint_crosshair(ui.painter(), rect.center(), &self.settings);
            });
        });
    }

    /// Closing the editor also closes an overlay started from it.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.kill_overlay();
    }
}

pub fn run() -> eframe::Result {
    eframe::run_native(
        "vibehair",
        NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("vibehair")
                .with_inner_size([560.0, 420.0])
                .with_min_inner_size([420.0, 320.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(EditorApp::new(cc)))),
    )
}
