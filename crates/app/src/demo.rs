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
            // /slots is polled while the page is read; only the POST gets the stream
            for conn in l.incoming() {
                let Ok(mut s) = conn else { return };
                std::thread::spawn(move || {
                    let mut b = [0u8; 65536];
                    let n = s.read(&mut b).unwrap_or(0);
                    if b[..n].starts_with(b"GET ") {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
                        return;
                    }
                    // the request is a JSON body with a JPEG in it: read until the client has stopped sending
                    s.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
                    while s.read(&mut b).map_or(false, |n| n > 0) {}
                    let r = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"```\\nHello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\\n```\"}}],\"usage\":{\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
                    s.write_all(r.as_bytes()).unwrap();
                });
            }
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

    /// Phase 3: the judge model's likely words come back from the server, cleaned like `W.parseGuesses`.
    #[test]
    fn guesses_are_asked_of_the_server_outside_demo_mode() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
            let mut b = [0u8; 65536];
            while s.read(&mut b).map_or(false, |n| n > 0) {}
            let content = r#"Sure: [{"n":1,"options":[{"text":"discriminant","percent":60},{"text":"determinant","percent":25}]}]"#;
            let j = serde_json::json!({"choices": [{"message": {"content": content}}]}).to_string();
            let r = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{j}", j.len());
            s.write_all(r.as_bytes()).unwrap();
        });
        let (store, slug) = block_on(seed(1)).unwrap();
        let be = Backend::new(store, "Saved");
        let conn = vishion_ui::Conn { url, timeout_secs: 5, ..Default::default() };
        let ev = block_on(be.handle(Cmd::Guess { slug, id: "p-maya-1".into(), raw: "the [illegible] root".into(), conn, examples: vec![] }, 0));
        let Event::Guessed { result: Ok(g), .. } = ev else { panic!("{ev:?}") };
        assert_eq!(g["i:0"][0]["text"], "discriminant");
        assert_eq!(g["i:0"][1]["percent"], 25);
    }

    /// `autoGuess`: a page read from the server that has an unreadable word is asked again for likely words.
    #[test]
    fn a_page_with_unreadable_words_gets_guesses_after_reading() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut s) = conn else { return };
                std::thread::spawn(move || {
                    s.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
                    let mut b = [0u8; 65536];
                    let mut req = Vec::new();
                    while let Ok(n) = s.read(&mut b) {
                        if n == 0 {
                            break;
                        }
                        req.extend_from_slice(&b[..n]);
                    }
                    let req = String::from_utf8_lossy(&req).to_string();
                    let reply = if req.starts_with("GET ") {
                        "HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_string()
                    } else if req.contains("\"stream\":true") {
                        "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"The [illegible] root\"}}],\"usage\":{\"completion_tokens\":3}}\n\ndata: [DONE]\n\n".to_string()
                    } else {
                        let c = r#"[{"n":1,"options":[{"text":"discriminant","percent":70}]}]"#;
                        let j = serde_json::json!({"choices": [{"message": {"content": c}}]}).to_string();
                        format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{j}", j.len())
                    };
                    let _ = s.write_all(reply.as_bytes());
                });
            }
        });
        let (store, slug) = block_on(seed(1)).unwrap();
        let be = Backend::new(store, "Saved");
        let conn = vishion_ui::Conn { url: url.clone(), timeout_secs: 5, guess: true, ..Default::default() };
        let Event::Many(evs) = block_on(be.handle(Cmd::Transcribe { slug: slug.clone(), ids: vec!["p-maya-1".into()], conn }, 0)) else { panic!() };
        let g = evs.iter().find_map(|e| match e {
            Event::Transcribed { outcome: vishion_ui::ReadOutcome::Done { guesses, .. }, .. } => Some(guesses.clone()),
            _ => None,
        });
        assert_eq!(g.flatten().unwrap()["i:0"][0]["text"], "discriminant");
        // switched off: the page is read and left alone
        let conn = vishion_ui::Conn { url, timeout_secs: 5, guess: false, ..Default::default() };
        let Event::Many(evs) = block_on(be.handle(Cmd::Transcribe { slug, ids: vec!["p-maya-1".into()], conn }, 0)) else { panic!() };
        assert!(evs.iter().any(|e| matches!(e, Event::Transcribed { outcome: vishion_ui::ReadOutcome::Done { guesses: None, .. }, .. })));
    }

    /// Import folder: a stored assignment folder is read back with its folder name in front, and imported
    /// as a new assignment under a fresh slug.
    #[test]
    fn a_project_folder_imports_as_a_new_assignment() {
        let dir = std::env::temp_dir().join(format!("vishion-folder-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = AnyStore::Fs(vishion_store::fs::FsStore::new(&dir));
        let (_store, slug) = block_on(seed_into(store, 1_700_000_000_000)).unwrap();
        let files = crate::backend::read_folder(&dir.join(&slug));
        assert!(files.iter().any(|f| f.name == format!("{slug}/project.json")), "{:?}", files.iter().map(|f| &f.name).collect::<Vec<_>>());
        let (target, _) = block_on(seed(1)).unwrap();
        let be = Backend::new(target, "Saved");
        let entries: Vec<(String, Vec<u8>)> = files.into_iter().map(|f| (f.name, f.bytes)).collect();
        let new_slug = block_on(be.projects.import_files(&entries)).unwrap();
        let Event::Opened { assignment, .. } = block_on(be.open(Some(&new_slug), 5)) else { panic!("open") };
        assert!(assignment.page("p-maya-1").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Old sessions: a session with text and a picture becomes a page of a new assignment named like the old
    /// app's import, with its applied corrections kept as resolved.
    #[test]
    fn old_sessions_import_as_a_new_assignment() {
        fn b64(data: &[u8]) -> String {
            const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for c in data.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                for i in 0..4 {
                    if i <= c.len() {
                        out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                    } else {
                        out.push('=');
                    }
                }
            }
            out
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(40, 30, image::Rgb([250, 250, 250])))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let url = format!("data:image/png;base64,{}", b64(&png.into_inner()));
        let sessions: Vec<serde_json::Value> = vec![
            serde_json::json!({"id": "s1", "name": "page one", "lastRaw": "The derivitive", "preRaw": "The derivitive", "created": 2,
                "appliedDiff": [{"orig": "derivitive", "term": "derivative"}], "topic": "Medical"}),
            serde_json::json!({"id": "s2", "name": "page zero", "lastRaw": "Hello", "created": 1}),
        ];
        let images: std::collections::BTreeMap<String, String> = [("s1".to_string(), url)].into();
        let (store, _) = block_on(seed(1)).unwrap();
        let be = Backend::new(store, "Saved");
        let ev = block_on(be.import_legacy_from(&sessions, &images, 1_700_000_000_000));
        let Event::Many(evs) = ev else { panic!("{ev:?}") };
        let Some(Event::Opened { assignment, .. }) = evs.into_iter().find(|e| matches!(e, Event::Opened { .. })) else { panic!("not opened") };
        assert_eq!(assignment.meta.name(), "Imported from the old app");
        assert_eq!(assignment.meta.topic(), "Medical");
        assert_eq!(assignment.pages.len(), 2);
        let with_corr = assignment.pages.values().find(|p| p.raw() == "The derivitive").unwrap();
        assert_eq!(with_corr.status(), "done");
        assert_eq!(with_corr.resolved().get("c:0").and_then(|v| v.as_str()), Some("suggested"));
    }

    /// Phase 8: the provider is picked by the `provider` setting, so a mock answers with no server at all, for
    /// reading, name reading and guesses.
    #[test]
    fn a_mock_provider_answers_without_a_server() {
        let (store, slug) = block_on(seed(1)).unwrap();
        let be = Backend::new(store, "Saved");
        let conn = vishion_ui::Conn { url: "http://127.0.0.1:1".into(), timeout_secs: 5, provider: "mock".into(), ..Default::default() };
        let Event::Many(evs) = block_on(be.handle(Cmd::Transcribe { slug: slug.clone(), ids: vec!["p-maya-1".into()], conn: conn.clone() }, 0)) else { panic!() };
        assert!(evs.iter().any(|e| matches!(e, Event::Transcribed { outcome: vishion_ui::ReadOutcome::Done { raw, .. }, .. } if raw == "A mock transcript.")), "{evs:?}");
        // a guess is answered by the mock's canned reply: an empty list parses to no guesses
        let ev = block_on(be.handle(Cmd::Guess { slug, id: "p-maya-1".into(), raw: "a [illegible] word".into(), conn, examples: vec![] }, 0));
        let Event::Guessed { result: Ok(g), .. } = ev else { panic!("{ev:?}") };
        assert!(g.is_empty());
    }

    /// Settings, Test connection: a server that lists models, one that refuses the key, one that is not there.
    #[test]
    fn test_connection_reports_models_a_refused_key_and_a_dead_port() {
        use std::io::{Read, Write};
        fn serve(reply: &'static str) -> String {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", l.local_addr().unwrap());
            std::thread::spawn(move || {
                for conn in l.incoming() {
                    let Ok(mut s) = conn else { return };
                    let mut b = [0u8; 8192];
                    let _ = s.read(&mut b);
                    let _ = s.write_all(reply.as_bytes());
                }
            });
            url
        }
        let ask = |conn: vishion_ui::Conn| -> (bool, String) {
            let be = Backend::new(AnyStore::Mem(vishion_store::mem::MemStore::default()), "Saved");
            match block_on(be.test_connection(&conn)) {
                Event::ConnectionTested { ok, text } => (ok, text),
                e => panic!("{e:?}"),
            }
        };
        let two = r#"{"data":[{"id":"a"},{"id":"b"}]}"#;
        let url = serve(Box::leak(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{two}", two.len()).into_boxed_str()));
        assert_eq!(ask(vishion_ui::Conn { url, ..Default::default() }), (true, "Connected. 2 models available.".to_string()));
        let url = serve("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(ask(vishion_ui::Conn { url, ..Default::default() }), (false, "The server did not accept the API key.".to_string()));
        let (ok, text) = ask(vishion_ui::Conn { url: "http://127.0.0.1:1".into(), ..Default::default() });
        assert!(!ok && text.starts_with("Can\u{2019}t reach http://127.0.0.1:1"), "{text}");
        assert_eq!(ask(vishion_ui::Conn { url: String::new(), ..Default::default() }), (false, "Enter the server address first.".to_string()));
        // the mock provider needs no server
        let m = ask(vishion_ui::Conn { url: "http://unused".into(), provider: "mock".into(), ..Default::default() });
        assert_eq!(m, (true, "Connected. 1 model available.".to_string()));
        assert!(ask(vishion_ui::Conn { demo: true, ..Default::default() }).1.starts_with("Demo mode is on"));
    }

    /// Settings, Find models: a local Ollama's models, with /api/show telling which can see images; hosted ones
    /// stay out of the local picker and text-only ones out of every picker.
    #[test]
    fn find_models_lists_the_vision_models_of_a_local_ollama() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut s) = conn else { return };
                std::thread::spawn(move || {
                    s.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
                    let mut req = Vec::new();
                    let mut b = [0u8; 8192];
                    while let Ok(n) = s.read(&mut b) {
                        if n == 0 {
                            break;
                        }
                        req.extend_from_slice(&b[..n]);
                        if req.windows(4).any(|w| w == b"\r\n\r\n") && (String::from_utf8_lossy(&req).starts_with("GET ") || req.ends_with(b"}")) {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&req).to_string();
                    let body = if text.starts_with("GET /v1/models") {
                        r#"{"data":[{"id":"llava:7b"},{"id":"coder:7b"},{"id":"gemma4:31b-cloud"}]}"#.to_string()
                    } else if text.contains("llava:7b") || text.contains("gemma4:31b-cloud") {
                        r#"{"capabilities":["completion","vision"]}"#.to_string()
                    } else {
                        r#"{"capabilities":["completion"]}"#.to_string()
                    };
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
                });
            }
        });
        let be = Backend::new(AnyStore::Mem(vishion_store::mem::MemStore::default()), "Saved");
        let ids = |ev: Event| -> Vec<String> {
            match ev {
                Event::ModelsFound(Ok(v)) => v.into_iter().map(|m| m.0).collect(),
                e => panic!("{e:?}"),
            }
        };
        let local = vishion_ui::Conn { url: url.clone(), provider: "local".into(), ..Default::default() };
        assert_eq!(ids(block_on(be.find_models(&local, None, false))), ["llava:7b"]);
        // another provider keeps the hosted model (and has no /api/show: the listing says nothing about images)
        let custom = vishion_ui::Conn { url, provider: "custom".into(), ..Default::default() };
        let mut got = ids(block_on(be.find_models(&custom, None, false)));
        got.sort();
        assert_eq!(got, ["coder:7b", "gemma4:31b-cloud", "llava:7b"]);
        // a working list narrows a cloud picker; asking for all ignores it
        assert_eq!(ids(block_on(be.find_models(&custom, Some(vec!["llava:7b".into()]), false))), ["llava:7b"]);
        assert_eq!(ids(block_on(be.find_models(&custom, Some(vec!["llava:7b".into()]), true))).len(), 3);
        // errors are sentences
        let dead = vishion_ui::Conn { url: "http://127.0.0.1:1".into(), provider: "custom".into(), ..Default::default() };
        match block_on(be.find_models(&dead, None, false)) {
            Event::ModelsFound(Err(m)) => assert!(m.starts_with("Can\u{2019}t reach"), "{m}"),
            e => panic!("{e:?}"),
        }
        let empty = vishion_ui::Conn::default();
        assert!(matches!(block_on(be.find_models(&empty, None, false)), Event::ModelsFound(Err(m)) if m == "Enter the server address first."));
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
