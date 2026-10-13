//! Native entry point. `vishion --exit-after-ms N` closes the window after N ms (smoke tests / CI).
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut exit_after = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--exit-after-ms" => match args.next().and_then(|v| v.parse::<u64>().ok()) {
                Some(ms) => exit_after = Some(std::time::Duration::from_millis(ms)),
                None => {
                    eprintln!("--exit-after-ms needs a number");
                    std::process::exit(2);
                }
            },
            other => {
                eprintln!("unknown argument {other:?}\nusage: vishion [--exit-after-ms N]");
                std::process::exit(2);
            }
        }
    }
    eframe::run_native(
        "viSHion",
        Default::default(),
        Box::new(move |cc| {
            let mut app = vishion_ui::App::new(cc.egui_ctx.clone());
            if let Some(d) = exit_after {
                app = app.exit_after(d);
            }
            Ok(Box::new(app))
        }),
    )
}

/// The web build runs `start()` from the lib; the bin target only has to compile on wasm32 so that
/// `cargo build --workspace --target wasm32-unknown-unknown` works.
#[cfg(target_arch = "wasm32")]
fn main() {}
