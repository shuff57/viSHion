//! A sample assignment for `--demo` / `?demo=1`: written through the real project layer into an in-memory
//! store, then opened like any other assignment (nothing about the screen is special-cased). The page
//! images are synthetic handwriting-like scribbles so the page pane, line marks and crop have something
//! to show without shipping a real student's work.
use crate::backend::AnyStore;
use serde_json::{json, Value};
use vishion_core::judge;
use vishion_core::model::Assignment;
use vishion_store::mem::MemStore;
use vishion_store::projects::Projects;
use vishion_store::Result;

const W: u32 = 900;
const H: u32 = 1160;
const TOP: u32 = 150;
const PITCH: u32 = 62;

/// One scribble per word along a baseline, wobbling like a pen. Deterministic.
fn page_jpeg(lines: &[&str]) -> Vec<u8> {
    let mut img = image::RgbImage::from_pixel(W, H, image::Rgb([252, 252, 248]));
    let mut y = 40;
    while y < H {
        for x in 0..W {
            img.put_pixel(x, y, image::Rgb([195, 212, 234]));
        }
        y += 31;
    }
    for y in 0..H {
        img.put_pixel(86, y, image::Rgb([231, 163, 163]));
        img.put_pixel(87, y, image::Rgb([231, 163, 163]));
    }
    let ink = image::Rgb([38, 53, 143]);
    let mut seed = 7u32;
    let mut rnd = |n: u32| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 16) % n
    };
    for (i, line) in lines.iter().enumerate() {
        let base = TOP + i as u32 * PITCH + 30;
        let mut x = 110u32;
        for word in line.split_whitespace() {
            let w = (word.chars().count() as u32 * 9).clamp(10, 200);
            for dx in 0..w {
                let wob = ((dx as f32 * 0.55).sin() * 7.0 + (dx as f32 * 0.17).cos() * 4.0) as i32;
                let top = base as i32 - 16 + wob - (rnd(5) as i32);
                for t in 0..3 {
                    let yy = (top + t + (dx as i32 % 3) * 3).clamp(0, H as i32 - 1) as u32;
                    if x + dx < 100 + 660 {
                        img.put_pixel(x + dx, yy, ink);
                        img.put_pixel(x + dx, (yy + 1).min(H - 1), ink);
                    }
                }
            }
            x += w + 14;
            if x > 100 + 600 {
                break;
            }
        }
    }
    let mut out = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(&img).expect("jpeg encode");
    out
}

fn boxes(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(|i| {
                json!({
                    "x": 100.0 / W as f64,
                    "y": (TOP + i as u32 * PITCH) as f64 / H as f64,
                    "w": 660.0 / W as f64,
                    "h": 44.0 / H as f64,
                    "r": 0
                })
            })
            .collect(),
    )
}

struct Seed {
    id: &'static str,
    name: &'static str,
    who: &'static str,
    status: &'static str,
    raw: &'static str,
    error: &'static str,
}

/// Write the sample assignment into a fresh in-memory store. Returns the store and the project slug.
pub async fn seed(now: i64) -> Result<(AnyStore, String)> {
    seed_into(AnyStore::Mem(MemStore::new()), now).await
}

/// Write the sample assignment through the project layer into `store` (any backend).
pub async fn seed_into(store: AnyStore, now: i64) -> Result<(AnyStore, String)> {
    let p = Projects::new(store);
    let slug = p.create_project("Unit 5 written response", now).await?;
    let seeds = [
        Seed {
            id: "p-maya-1",
            name: "maya-1.jpg",
            who: "Maya R.",
            status: "done",
            raw: "The 95% confidance interval for the average commute time is (18.4, 24.1) minutes.\n\
                  $\\bar{x} \\pm 1.96 \\cdot \\frac{s}{\\sqrt{n}} = 21.25 \\pm 1.96 \\cdot \\frac{9.2}{\\sqrt{40}}$\n\
                  $$\\bar{x} \\pm 1.96\\cdot\\frac{s}{\\sqrt{n}} = (18.4,\\ 24.1)$$\n\
                  It does not mean that 95% of students commute in that range. If we took many samples,\n\
                  about 95% of those [illegible] would capture the true populaton mean.\n\
                  ~~probably~~ so the interval is likely wide.\n\
                  Margin: check my units!",
            error: "",
        },
        Seed {
            id: "p-maya-2",
            name: "maya-2.jpg",
            who: "Maya R.",
            status: "done",
            raw: "A wider interval means we know the mean less precisely.\n\
                  A larger sample gives a narrower interval, because the standerd error gets smaller as n goes up.",
            error: "",
        },
        Seed {
            id: "p-ana-1",
            name: "ana-1.jpg",
            who: "Ana L.",
            status: "done",
            raw: "The sample mean is 21.25 minutes.\nThere is nothing wrong with this page.",
            error: "",
        },
        Seed { id: "p-ben-1", name: "ben-1.jpg", who: "Ben T.", status: "pending", raw: "", error: "" },
        Seed {
            id: "p-omar-1",
            name: "omar-1.jpg",
            who: "Omar K.",
            status: "error",
            raw: "",
            error: "The transcription server did not answer.",
        },
    ];
    let glossary: Vec<String> =
        ["confidence", "population", "standard", "deviation"].iter().map(|s| s.to_string()).collect();
    let mut order = vec![];
    for s in &seeds {
        let mut page = Assignment::new_page(s.id, s.name);
        page.set("status", json!(s.status));
        page.set("raw", json!(s.raw));
        page.set("error", json!(s.error));
        page.set("student", json!({"name": s.who, "source": "page", "confirmed": true}));
        let lines = vishion_core::render::units(s.raw);
        page.set("boxes", boxes(lines.len()));
        if s.status == "done" {
            // the judge pass the transcription queue runs after reading a page
            let found = judge::heuristic(s.raw, &glossary);
            judge::apply_found(&mut page, &found);
            page.set("timings", json!({"ms": 14200, "tokens": 212}));
        }
        if s.id == "p-maya-1" {
            page.set("guesses", json!({"i:0": [{"text": "intervals", "percent": 62}, {"text": "intervening", "percent": 21}]}));
        }
        let texts: Vec<&str> = if lines.is_empty() { vec!["..."] } else { lines.iter().map(|l| l.as_str()).collect() };
        p.save_image(&slug, s.id, &page_jpeg(&texts)).await?;
        p.save_page(&slug, &page).await?;
        order.push(s.id.to_string());
    }
    let mut meta = p.meta(&slug, now).await?.expect("project just created");
    meta.set("topic", json!("Math/Education"));
    meta.set_order(&order);
    meta.set("stats", json!({"pages": seeds.len(), "done": 3}));
    meta.set("ui", json!({"selectedId": "p-maya-1"}));
    p.save_meta(&slug, &mut meta, Some(now)).await?;
    Ok((p.store, slug))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{block_on, Backend};
    use vishion_ui::{Cmd, Event};

    #[test]
    fn demo_opens_through_the_project_layer_with_judge_output() {
        let (store, slug) = block_on(seed(1_700_000_000_000)).unwrap();
        let be = Backend::new(store, "Demo");
        let ev = block_on(be.open(None, 1_700_000_000_000));
        let Event::Opened { slug: s, assignment, .. } = ev else { panic!("expected Opened, got {ev:?}") };
        assert_eq!(s, slug);
        let maya = assignment.page("p-maya-1").unwrap();
        // judge::heuristic produced the corrections the review screen turns into flagged words
        let words: Vec<String> = vishion_core::words::items(maya).iter().map(|i| i.key.clone()).collect();
        assert!(words.contains(&"c:0".to_string()) && words.contains(&"c:1".to_string()), "{words:?}");
        assert!(words.contains(&"i:0".to_string()));
        assert_eq!(assignment.meta.order().len(), 5);
        let img = block_on(be.handle(vishion_ui::Cmd::LoadImage { slug, id: "p-maya-1".into() }, 0));
        assert!(matches!(img, Event::Image { bytes: Some(b), .. } if b.len() > 1000));
    }

    /// The native path: a real folder, written and read back through `FsStore` and the project layer.
    #[test]
    fn folder_store_round_trip_keeps_edits() {
        let dir = std::env::temp_dir().join(format!("vishion-app-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = AnyStore::Fs(vishion_store::fs::FsStore::new(&dir));
        let (store, slug) = block_on(seed_into(store, 1_700_000_000_000)).unwrap();
        let be = Backend::new(store, "Saved to your folder");
        let Event::Opened { mut assignment, .. } = block_on(be.open(Some(&slug), 1)) else { panic!("open") };
        let mut page = assignment.pages.remove("p-maya-1").unwrap();
        let mut settings = vishion_core::model::Settings::defaults();
        assert!(vishion_core::words::choose(&mut page, "c:0", "suggested", "", &mut settings, "Math/Education", 5));
        assert!(matches!(block_on(be.handle(Cmd::SavePage { slug: slug.clone(), page }, 9)), Event::Saved));
        let mut meta = assignment.meta.clone();
        meta.set("ui", serde_json::json!({"selectedId": "p-maya-2"}));
        assert!(matches!(block_on(be.handle(Cmd::SaveMeta { slug: slug.clone(), meta }, 10)), Event::Saved));
        // a fresh backend over the same folder sees both
        let be2 = Backend::new(AnyStore::Fs(vishion_store::fs::FsStore::new(&dir)), "Saved to your folder");
        let Event::Opened { assignment: a2, .. } = block_on(be2.open(None, 20)) else { panic!("reopen") };
        let p = a2.page("p-maya-1").unwrap();
        assert_eq!(p.resolved().get("c:0").and_then(|v| v.as_str()), Some("suggested"));
        assert!(p.corrected().contains("confidence"));
        assert_eq!(a2.meta.get("ui").unwrap()["selectedId"], "p-maya-2");
        assert!(dir.join(&slug).join("project.json.bak").exists(), "saveMeta keeps the previous project.json");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
