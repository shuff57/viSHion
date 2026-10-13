//! Native entry point.
//!
//! ```text
//! vishion [--root DIR] [--project SLUG] [--demo | --seed-demo-to DIR] [--theme light|dark]
//!         [--size WxH] [--exit-after-ms N] [--screenshot FILE.png] [--dump-controls]
//! ```
//! `--root` is the folder that holds the assignments (default: `$VISHION_DIR`, else `~/Documents/viSHion`).
//! `--demo` runs the sample assignment from memory; `--seed-demo-to DIR` writes it into DIR first and opens
//! that folder (edits persist there). `--exit-after-ms` closes the window cleanly after N ms
//! (smoke tests / CI); `--size 390x844` opens a phone-sized window; `--screenshot` writes the first settled frame and closes; `--dump-controls` prints
//! the control list (id, role, label, size) as JSON lines, to compare with the web twin.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use vishion_app::backend::{block_on, AnyStore, Backend};
    use vishion_store::fs::FsStore;
    use vishion_ui::{Cmd, Inbox};

    let mut root: Option<PathBuf> = None;
    let (mut project, mut demo, mut theme) = (None::<String>, false, None::<String>);
    let mut inbox = false;
    let mut page_id = None::<String>;
    let mut seed_to: Option<PathBuf> = None;
    let (mut exit_after, mut shot, mut dump) = (None, None::<PathBuf>, false);
    let mut size = (1280.0f32, 800.0f32);
    let mut args = std::env::args().skip(1);
    let usage = "usage: vishion [--root DIR] [--project SLUG] [--demo] [--seed-demo-to DIR] [--theme light|dark] [--size WxH] [--inbox] [--page ID] [--exit-after-ms N] [--screenshot FILE.png] [--dump-controls]";
    let need = |a: Option<String>, flag: &str| -> String {
        a.unwrap_or_else(|| {
            eprintln!("{flag} needs a value\n{usage}");
            std::process::exit(2)
        })
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--exit-after-ms" => match need(args.next(), &a).parse::<u64>() {
                Ok(ms) => exit_after = Some(std::time::Duration::from_millis(ms)),
                Err(_) => {
                    eprintln!("--exit-after-ms needs a number");
                    std::process::exit(2);
                }
            },
            "--size" => {
                let v = need(args.next(), &a);
                let parsed = v.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)));
                match parsed {
                    Some(wh) => size = wh,
                    None => {
                        eprintln!("--size needs WIDTHxHEIGHT, e.g. 390x844");
                        std::process::exit(2);
                    }
                }
            }
            "--root" => root = Some(PathBuf::from(need(args.next(), &a))),
            "--project" => project = Some(need(args.next(), &a)),
            "--theme" => theme = Some(need(args.next(), &a)),
            "--screenshot" => shot = Some(PathBuf::from(need(args.next(), &a))),
            "--demo" => demo = true,
            "--inbox" => inbox = true,
            "--page" => page_id = Some(need(args.next(), &a)),
            "--seed-demo-to" => seed_to = Some(PathBuf::from(need(args.next(), &a))),
            "--dump-controls" => dump = true,
            other => {
                eprintln!("unknown argument {other:?}\n{usage}");
                std::process::exit(2);
            }
        }
    }
    let root = root.or_else(|| std::env::var_os("VISHION_DIR").map(PathBuf::from)).unwrap_or_else(|| {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
        home.join("Documents").join("viSHion")
    });

    // Window size: the screenshot/dump runs use the design's reference size.
    let mut native = eframe::NativeOptions::default();
    native.viewport = egui_viewport(size.0, size.1);

    eframe::run_native(
        "viSHion",
        native,
        Box::new(move |cc| {
            let inbox_box = Inbox::new(cc.egui_ctx.clone());
            let (tx, rx) = mpsc::channel::<Cmd>();
            let worker_inbox = inbox_box.clone();
            std::thread::spawn(move || {
                let now = vishion_ui::now_ms;
                let backend = if let Some(dir) = &seed_to {
                    let fs = AnyStore::Fs(FsStore::new(dir));
                    match block_on(vishion_app::demo::seed_into(fs, now())) {
                        Ok((store, _)) => Backend::new(store, "Saved to your folder"),
                        Err(e) => {
                            worker_inbox.push(vishion_ui::Event::Failed(e.to_string()));
                            return;
                        }
                    }
                } else if demo {
                    match block_on(vishion_app::demo::seed(now())) {
                        Ok((store, _)) => Backend::new_demo(store, "Demo assignment (not saved)"),
                        Err(e) => {
                            worker_inbox.push(vishion_ui::Event::Failed(e.to_string()));
                            return;
                        }
                    }
                } else {
                    Backend::new(AnyStore::Fs(FsStore::new(&root)), "Saved to your folder")
                };
                worker_inbox.push(block_on(backend.open(project.as_deref(), now())));
                while let Ok(cmd) = rx.recv() {
                    worker_inbox.push(block_on(backend.handle(cmd, now())));
                }
            });
            let dispatch: vishion_ui::Dispatch = Box::new(move |c| {
                let _ = tx.send(c);
            });
            let mut app = vishion_ui::App::new(cc.egui_ctx.clone()).with_host(dispatch, inbox_box).inbox(inbox);
            if let Some(t) = &theme {
                app = app.theme(t);
            }
            if let Some(p) = &page_id {
                app = app.page(p);
            }
            if let Some(d) = exit_after {
                app = app.exit_after(d);
            }
            if let Some(p) = shot {
                app = app.screenshot_to(p, 1.0);
            }
            Ok(Box::new(app.dump_controls(dump)))
        }),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn egui_viewport(w: f32, h: f32) -> eframe::egui::ViewportBuilder {
    eframe::egui::ViewportBuilder::default().with_inner_size([w, h])
}

/// The web build runs `start()` from the lib; the bin target only has to compile on wasm32 so that
/// `cargo build --workspace --target wasm32-unknown-unknown` works.
#[cfg(target_arch = "wasm32")]
fn main() {}
