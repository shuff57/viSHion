//! Entry points. Native uses the `vishion` binary (src/main.rs); the web build exports `start(canvas_id)`.
//! This crate stays thin: all behaviour lives in core/store/net/ui.

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// Mount the egui app on the canvas with the given id.
    #[wasm_bindgen]
    pub async fn start(canvas_id: String) -> Result<(), JsValue> {
        eframe::WebLogger::init(log::LevelFilter::Warn).ok();
        eframe::WebRunner::new()
            .start(
                &canvas_id,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(vishion_ui::App::new(cc.egui_ctx.clone())))),
            )
            .await?;
        // eframe sets tabindex=0 on the canvas and appends an unnamed IME <input>; fix both (keyboard path).
        vishion_ui::harden_web_dom(&canvas_id);
        Ok(())
    }
}
