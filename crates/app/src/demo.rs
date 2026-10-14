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

    fn png(w: u32, h: u32) -> Vec<u8> {
        let im = image::RgbImage::from_pixel(w, h, image::Rgb([20, 80, 160]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(im).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn dims(b: &[u8]) -> (u32, u32) {
        let i = image::load_from_memory(b).unwrap();
        (i.width(), i.height())
    }

    #[test]
    fn rotate_turns_the_stored_image_and_answers_with_the_new_bytes() {
        let (store, slug) = block_on(seed(1_700_000_000_000)).unwrap();
        let be = Backend::new(store, "Demo");
        let before = match block_on(be.handle(Cmd::LoadImage { slug: slug.clone(), id: "p-maya-1".into() }, 0)) {
            Event::Image { bytes: Some(b), .. } => dims(&b),
            e => panic!("{e:?}"),
        };
        // the answer is the new picture and the end of the save (the header was left at "Saving..." without it)
        let Event::Many(mut both) = block_on(be.handle(Cmd::RotateImage { slug: slug.clone(), id: "p-maya-1".into() }, 0)) else {
            panic!("rotate")
        };
        assert!(matches!(both.pop(), Some(Event::Saved)));
        let Some(Event::Image { id, bytes: Some(b) }) = both.pop() else { panic!("rotate image") };
        assert_eq!(id, "p-maya-1");
        assert_eq!(dims(&b), (before.1, before.0));
        // stored: a second load sees the turned picture
        let Event::Image { bytes: Some(again), .. } = block_on(be.handle(Cmd::LoadImage { slug, id: "p-maya-1".into() }, 0)) else { panic!() };
        assert_eq!(dims(&again), (before.1, before.0));
    }

    #[test]
    fn remove_deletes_the_page_and_its_image() {
        let (store, slug) = block_on(seed(1_700_000_000_000)).unwrap();
        let be = Backend::new(store, "Demo");
        assert!(matches!(block_on(be.handle(Cmd::RemovePage { slug: slug.clone(), id: "p-maya-2".into() }, 0)), Event::Saved));
        let img = block_on(be.handle(Cmd::LoadImage { slug: slug.clone(), id: "p-maya-2".into() }, 0));
        assert!(matches!(img, Event::Image { bytes: None, .. }));
        let Event::Opened { assignment, .. } = block_on(be.open(Some(&slug), 1)) else { panic!("open") };
        assert!(assignment.page("p-maya-2").is_none());
    }

    #[test]
    fn replace_and_add_store_pictures_and_hand_pages_to_the_ui() {
        use crate::backend::Picked;
        let (store, slug) = block_on(seed(1_700_000_000_000)).unwrap();
        let be = Backend::new(store, "Demo");
        let ev = block_on(be.replace_with(&slug, "p-maya-1", Picked { name: "retake.png".into(), bytes: png(2400, 1800) }));
        let Event::PageReplaced { id, name, bytes } = ev else { panic!("{ev:?}") };
        assert_eq!((id.as_str(), name.as_str()), ("p-maya-1", "retake.png"));
        assert_eq!(dims(&bytes), (1600, 1200), "shrunk to 1600 on the long side");
        // a bad file is a notice, not a change
        let bad = block_on(be.replace_with(&slug, "p-maya-1", Picked { name: "x.txt".into(), bytes: b"hello".to_vec() }));
        assert!(matches!(bad, Event::Notice { error: true, .. }));
        // two pictures for a student, one unreadable
        let ev = block_on(be.add_with(
            &slug,
            "Maya R.",
            vec![
                Picked { name: "a.png".into(), bytes: png(300, 400) },
                Picked { name: "junk.png".into(), bytes: vec![1, 2, 3] },
                Picked { name: "b.png".into(), bytes: png(300, 400) },
            ],
            1_700_000_000_500,
        ));
        let Event::PagesAdded { pages } = ev else { panic!("{ev:?}") };
        assert_eq!(pages.len(), 2);
        assert_ne!(pages[0].0.id(), pages[1].0.id(), "ids are unique");
        assert_eq!(pages[0].0.student_name(), Some("Maya R."));
        let Event::Image { bytes: Some(b), .. } =
            block_on(be.handle(Cmd::LoadImage { slug, id: pages[0].0.id().to_string() }, 0)) else { panic!("image") };
        assert_eq!(dims(&b), (300, 400));
    }

    #[test]
    fn uid_is_base36_time_plus_a_tail() {
        let a = crate::backend::uid(1_700_000_000_000, 0);
        let b = crate::backend::uid(1_700_000_000_000, 1);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()) && a.len() >= 13, "{a}");
        assert!(a.starts_with("lo"), "base36 of 1.7e12 is lo...: {a}");
    }

    #[test]
    fn straighten_levels_and_transcribe_reads_in_demo_mode() {
        let (store, slug) = block_on(seed(1)).unwrap();
        let be = Backend::new_demo(store, "Demo");
        let s = block_on(be.handle(Cmd::Straighten { slug: slug.clone(), id: "p-maya-1".into() }, 0));
        assert!(matches!(s, Event::Straightened { angle, bytes: None, .. } if angle == 0.0), "{s:?}");
        // demo mode answers with the sample text, one event per page, by position in the run
        let Event::Many(evs) = block_on(be.handle(Cmd::Transcribe { slug: slug.clone(), ids: vec!["p-omar-1".into(), "p-ben-1".into()], conn: Default::default() }, 0)) else {
            panic!("expected one event per page")
        };
        assert_eq!(evs.len(), 2);
        match &evs[0] {
            Event::Transcribed { id, outcome: vishion_ui::ReadOutcome::Done { raw, guesses, .. } } => {
                assert_eq!(id, "p-omar-1");
                assert!(raw.starts_with("Lesson 4: Quadratics"), "{raw}");
                assert_eq!(guesses.as_ref().map(|g| g.len()), Some(1));
            }
            e => panic!("{e:?}"),
        }
        assert!(matches!(&evs[1], Event::Transcribed { outcome: vishion_ui::ReadOutcome::Done { raw, .. }, .. } if raw.starts_with("Pt seen")));
        // without demo mode the page is left alone and the message says why
        let (store, slug) = block_on(seed(1)).unwrap();
        let be = Backend::new(store, "Saved");
        let Event::Many(evs) = block_on(be.handle(Cmd::Transcribe { slug, ids: vec!["p-maya-1".into()], conn: Default::default() }, 0)) else { panic!() };
        assert!(evs.iter().any(|e| matches!(e, Event::Transcribed { outcome: vishion_ui::ReadOutcome::Unreachable(m), .. } if m.contains("OCR"))));
    }

    /// Phase 3 end to end: a page is read from a (local) chat-completions server, streamed into `Reading`
    /// events, cleaned, and answered as `Done`; Stop ends a run.
    #[test]
    fn a_page_is_read_from_the_server_and_stop_ends_the_run() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            // the request is a JSON body with a JPEG in it: read until the client has stopped sending
            s.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
            let mut b = [0u8; 65536];
            while s.read(&mut b).map_or(false, |n| n > 0) {}
            let r = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"```\\nHello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\\n```\"}}],\"usage\":{\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
            s.write_all(r.as_bytes()).unwrap();
        });
        let (store, slug) = block_on(seed(1)).unwrap();
        let be = Backend::new(store, "Saved");
        let conn = vishion_ui::Conn { url, timeout_secs: 5, prompt: "Read.".into(), ..Default::default() };
        let Event::Many(evs) = block_on(be.handle(Cmd::Transcribe { slug: slug.clone(), ids: vec!["p-maya-1".into()], conn }, 0)) else { panic!() };
        assert!(evs.iter().any(|e| matches!(e, Event::Reading { text, .. } if text == "```\nHello world\n```")));
        assert!(evs.iter().any(|e| matches!(e, Event::Transcribed { outcome: vishion_ui::ReadOutcome::Done { raw, tokens: 2, .. }, .. } if raw == "Hello world")));

        // Stop before the run starts reading anything: nothing is sent to the server.
        be.cancel.stop();
        let out = std::cell::RefCell::new(vec![]);
        let conn = vishion_ui::Conn { url: "http://127.0.0.1:1".into(), timeout_secs: 5, ..Default::default() };
        // read_pages resets the flag at the start of a run (a Stop belongs to the run in progress)
        block_on(be.read_pages(&slug, vec!["p-maya-1".into()], conn, &|e| out.borrow_mut().push(e)));
        assert!(out.borrow().iter().any(|e| matches!(e, Event::Transcribed { outcome: vishion_ui::ReadOutcome::Unreachable(_), .. })));
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
