//! Startup selector: Open Editor / Launch Overlay.

use eframe::egui::CentralPanel;
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::{editor, overlay};

enum Next {
    None,
    Editor,
    Overlay,
}

pub struct LauncherApp {
    next: Next,
}

impl LauncherApp {
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        Self { next: Next::None }
    }
}

impl App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut EframeFrame) {
        CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("vibehair");
                ui.add_space(8.0);
                if ui.button("open editor").clicked() {
                    self.next = Next::Editor;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("launch overlay").clicked() {
                    self.next = Next::Overlay;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

pub fn run() -> eframe::Result {
    let chosen = std::rc::Rc::new(std::cell::Cell::new(0u8));
    let chosen_in = chosen.clone();
    eframe::run_native(
        "vibehair",
        NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("vibehair")
                .with_inner_size([260.0, 170.0])
                .with_resizable(false),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            let chosen_in = chosen_in.clone();
            Ok(Box::new(LauncherThen(chosen_in, LauncherApp::new(cc))) as Box<dyn App>)
        }),
    )?;
    match chosen.get() {
        1 => editor::run(),
        2 => overlay::run(),
        _ => Ok(()),
    }
}

/// Adapter: remembers the launcher choice, then closes.
struct LauncherThen(std::rc::Rc<std::cell::Cell<u8>>, LauncherApp);

impl App for LauncherThen {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut EframeFrame) {
        self.1.ui(ui, frame);
        match self.1.next {
            Next::Editor => self.0.set(1),
            Next::Overlay => self.0.set(2),
            Next::None => {}
        }
    }
}
