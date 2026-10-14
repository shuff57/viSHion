//! `vishion-s7` native: window by default, `--report` runs the corpus headless and prints math failures.
use vishion_s7::*;

fn main() -> eframe::Result<()> {
    if let Some(p) = std::env::args().skip_while(|a| a != "--tex").nth(1) {
        let mut cache = MathCache::new();
        for l in std::fs::read_to_string(p).unwrap().lines().filter(|l| !l.trim().is_empty()) {
            let m = Math { tex: l.into(), display: true };
            let ok = cache.get(&m, 16, egui::Color32::BLACK).is_some();
            println!("{} {}", if ok { "ok  " } else { "FAIL" }, l);
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--report") {
        let mut cache = MathCache::new();
        let (mut n_units, mut n_math, mut ok) = (0, 0, 0);
        for c in corpus() {
            for u in units(&c.text) {
                n_units += 1;
                let (held, math) = hold_math(&u);
                let _ = pieces(&held, math.len());
                for m in &math {
                    n_math += 1;
                    if cache.get(m, 16, egui::Color32::BLACK).is_some() { ok += 1; } else { println!("FAIL [{}] {:?}", c.name, m.tex); }
                }
            }
        }
        println!("units={n_units} math={n_math} ok={ok} fail={}", n_math - ok);
        return Ok(());
    }
    eframe::run_native("viSHion S7", Default::default(), Box::new(|_| Ok(Box::new(App::new()))))
}
