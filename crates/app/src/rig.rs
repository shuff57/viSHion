//! Test rig: the real screen over the real `Backend` (demo store), no window. Commands the UI sends are
//! answered synchronously through `block_on`, exactly as the native worker thread answers them, so a test
//! drives "click, host answers, screen updates" end to end. Used by the review-screen parity tests.
use crate::backend::{block_on, Backend};
use eframe::egui;
use std::rc::Rc;
use vishion_ui::{App, Cmd, Inbox};

pub struct Rig {
    pub app: App,
    pub ctx: egui::Context,
    pub out: egui::FullOutput,
    pub t: f64,
    pub size: (f32, f32),
    pub be: Rc<Backend>,
    pub slug: String,
}

impl Rig {
    /// The demo assignment (maya, ana, ben, omar), `theme` "light" or "dark".
    pub fn demo() -> Rig {
        Self::sized(1280.0, 800.0)
    }

    pub fn sized(w: f32, h: f32) -> Rig {
        let (store, slug) = block_on(crate::demo::seed(1_700_000_000_000)).expect("seed");
        let be = Rc::new(Backend::new_demo(store, "Demo assignment (not saved)"));
        let ctx = egui::Context::default();
        let inbox = Inbox::new(ctx.clone());
        let (b2, i2) = (be.clone(), inbox.clone());
        let dispatch: vishion_ui::Dispatch = Box::new(move |c: Cmd| {
            // picture dialogs and font fetches are not part of the rig
            if matches!(c, Cmd::LoadMathFont | Cmd::LoadFallbackFont | Cmd::ReplaceImage { .. } | Cmd::AddPages { .. }) {
                return;
            }
            i2.push(block_on(b2.handle(c, 1_700_000_000_000)));
        });
        let app = App::new(ctx.clone()).with_host(dispatch, inbox.clone()).theme("light");
        inbox.push(block_on(be.open(None, 1_700_000_000_000)));
        let mut r = Rig { app, ctx, out: Default::default(), t: 0.0, size: (w, h), be, slug };
        r.frames(8);
        r
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.t += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size.0, self.size.1))),
                time: Some(self.t),
                ..Default::default()
            };
            let app = &mut self.app;
            self.out = self.ctx.run(input, |c| app.run_frame(c));
        }
    }

    pub fn act(&mut self, id: &str) {
        assert!(self.app.controls().iter().any(|c| c.id == id), "no control {id}: {:?}", self.app.controls().iter().map(|c| &c.id).collect::<Vec<_>>());
        self.app.twin_act(id);
        self.frames(6);
    }

    pub fn has(&self, id: &str) -> bool {
        self.app.controls().iter().any(|c| c.id == id)
    }

    /// Every text drawn this frame with its top-left corner, in paint order.
    pub fn texts(&self) -> Vec<(String, f32, f32)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, f32, f32)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos.x, t.pos.y)),
                egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        for cs in &self.out.shapes {
            walk(&cs.shape, &mut out);
        }
        out
    }

    pub fn y_of(&self, text: &str) -> Option<f32> {
        self.texts().into_iter().find(|(t, _, _)| t == text).map(|(_, _, y)| y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints where everything sits on the demo's first screen (run with `--nocapture --ignored`).
    #[test]
    #[ignore]
    fn dump_layout() {
        let w: f32 = std::env::var("W").ok().and_then(|v| v.parse().ok()).unwrap_or(1280.0);
        let h: f32 = std::env::var("H").ok().and_then(|v| v.parse().ok()).unwrap_or(800.0);
        let mut r = Rig::sized(w, h);
        r.frames(20);
        for (t, x, y) in r.texts() {
            println!("{y:7.1} {x:7.1}  {t:?}");
        }
        for c in r.app.controls() {
            println!("ctl {} at={:?} size={:?}", c.id, c.at, c.size);
        }
    }
}
