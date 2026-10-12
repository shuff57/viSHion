//! viSHion UI. Phase 0 is a shell that proves the plumbing: egui on native and wasm, plus the `ctl()`
//! control layer (plan rule 3). Every interactive widget goes through `App::ctl`, which
//! (1) reports activation from egui OR from its hidden-DOM twin, (2) registers role/label for the twin,
//! (3) draws a focus ring when the twin has DOM focus. Twins are keyed by id and sorted by screen position (S8).
use egui::{Color32, Response, RichText, Stroke, Ui, Vec2};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
mod web;

/// Web only: call once after eframe started; makes the canvas a non-tabbable, AT-hidden surface and names
/// eframe's IME input. See `web.rs` for the keyboard model.
#[cfg(target_arch = "wasm32")]
pub fn harden_web_dom(canvas_id: &str) {
    web::harden(canvas_id);
}

const INK: Color32 = Color32::from_rgb(0x1B, 0x22, 0x30);

/// Messages from the DOM twin to egui: `act:<id>`, `focus:<id>`, `blur:<id>`.
pub type Queue = Arc<Mutex<Vec<String>>>;

#[derive(Clone, PartialEq, Debug)]
pub struct Reg {
    pub id: String,
    pub role: &'static str,
    pub label: String,
    pub pressed: Option<bool>,
    pub pos: (i32, i32),
}

pub struct App {
    #[allow(dead_code)]
    ctx: egui::Context,
    queue: Queue,
    act: HashSet<String>,
    focus: Option<String>,
    regs: Vec<Reg>,
    last_regs: Vec<Reg>,
    count: u32,
    exit_after: Option<Duration>,
}

impl App {
    pub fn new(ctx: egui::Context) -> Self {
        let queue: Queue = Arc::default();
        #[cfg(target_arch = "wasm32")]
        web::install(queue.clone(), ctx.clone());
        Self {
            ctx,
            queue,
            act: HashSet::new(),
            focus: None,
            regs: vec![],
            last_regs: vec![],
            count: 0,
            exit_after: None,
        }
    }

    /// Close the window after `d` (used by the native smoke test; `vishion --exit-after-ms N`).
    pub fn exit_after(mut self, d: Duration) -> Self {
        self.exit_after = Some(d);
        self
    }

    /// Controls registered in the last frame, in reading order. Exposed for tests.
    pub fn controls(&self) -> &[Reg] {
        &self.last_regs
    }

    /// The single funnel for interactive widgets.
    fn ctl(
        &mut self,
        ui: &Ui,
        r: &Response,
        id: &str,
        role: &'static str,
        label: &str,
        pressed: Option<bool>,
    ) -> bool {
        self.regs.push(Reg {
            id: id.into(),
            role,
            label: label.into(),
            pressed,
            pos: (r.rect.min.y as i32 / 16, r.rect.min.x as i32),
        });
        if self.focus.as_deref() == Some(id) {
            ui.painter()
                .rect_stroke(r.rect.expand(2.0), 6.0, Stroke::new(2.0, INK));
        }
        let from_dom = self.act.remove(id);
        r.clicked() || from_dom
    }

    fn drain_queue(&mut self) {
        let msgs: Vec<String> = std::mem::take(&mut *self.queue.lock().unwrap());
        for m in msgs {
            if let Some(id) = m.strip_prefix("act:") {
                self.act.insert(id.into());
            } else if let Some(id) = m.strip_prefix("focus:") {
                self.focus = Some(id.into());
            } else if let Some(id) = m.strip_prefix("blur:") {
                if self.focus.as_deref() == Some(id) {
                    self.focus = None;
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui) {
        ui.add_space(16.0);
        ui.heading("viSHion");
        ui.label(format!("core v{}", vishion_core::version()));
        ui.add_space(12.0);
        let r = ui.add_sized(
            [160.0, 44.0],
            egui::Button::new(RichText::new(format!("Count: {}", self.count)).strong())
                .min_size(Vec2::new(0.0, 44.0)),
        );
        if self.ctl(
            ui,
            &r,
            "count",
            "button",
            &format!("Count: {}", self.count),
            None,
        ) {
            self.count += 1;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_queue();
        self.regs.clear();
        egui::CentralPanel::default().show(ctx, |ui| self.ui(ui));

        // reading order = visual order (16px row bands, then x), so Tab matches what a sighted user sees
        self.regs.sort_by_key(|r| r.pos);
        if self.regs != self.last_regs {
            #[cfg(target_arch = "wasm32")]
            web::sync(&self.regs);
            self.last_regs = self.regs.clone();
        }

        if let Some(d) = self.exit_after {
            // egui time (not std Instant, which panics on wasm32)
            if ctx.input(|i| i.time) >= d.as_secs_f64() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dom_activation_increments_and_registers_control() {
        let ctx = egui::Context::default();
        let mut app = App::new(ctx.clone());
        app.queue.lock().unwrap().push("act:count".into());
        let frame_run = |app: &mut App| {
            let _ = ctx.run(Default::default(), |c| {
                app.drain_queue();
                app.regs.clear();
                egui::CentralPanel::default().show(c, |ui| app.ui(ui));
                app.last_regs = app.regs.clone();
            });
        };
        frame_run(&mut app);
        assert_eq!(app.count, 1);
        assert_eq!(app.controls().len(), 1);
        assert_eq!(app.controls()[0].id, "count");
    }
}
