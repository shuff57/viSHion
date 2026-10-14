fn main() -> eframe::Result<()> {
    eframe::run_native("viSHion S8 review", Default::default(), Box::new(|cc| Ok(Box::new(vishion_s8::App::new(cc.egui_ctx.clone())))))
}
