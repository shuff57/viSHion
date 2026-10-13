//! Project layer over any [`Store`]: js/fs.js (safe JSON writes) + js/fs-projects.js (list / open / save /
//! duplicate / delete / import) + the settings.json rules, with the same on-disk layout:
//!
//! ```text
//! settings.json                      pretty, secrets stripped
//! <slug>/project.json                pretty (2 spaces, no trailing newline), key order normalised
//! <slug>/project.json.bak            previous project.json, written before every save except creation
//! <slug>/pages/<id>.json             compact
//! <slug>/images/<id>.jpg
//! <slug>/originals/<id>.<ext>
//! <slug>/exports/<name>
//! ```
//!
//! Safety rules beyond the JS app (the point of the lenient path): documents with lone surrogates parse
//! and re-serialise byte-identically; a page that exists but cannot be parsed stays in `meta.order` and is
//! never rewritten; an unusable project.json is never overwritten.
use crate::{decode_text, Result, Store, StoreError};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use vishion_core::export::slug as make_slug;
use vishion_core::import::Plan;
use vishion_core::jsx;
use vishion_core::lenient;
use vishion_core::model::{Assignment, Page, Project, Settings};

/// Keys never written to settings.json and never read from it (API keys live in localStorage only).
pub const SETTINGS_STRIP: [&str; 4] = ["apiKey", "profiles", "storage", "noticeSeen"];
pub const SETTINGS_PATH: &str = "settings.json";

fn is_project_json(path: &str) -> bool {
    path == "project.json" || path.ends_with("/project.json")
}

fn parse_err(path: &str, e: impl ToString) -> StoreError {
    StoreError::Parse { path: path.into(), message: e.to_string() }
}

/// One row of the home screen (`fs.listProjects`).
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub slug: String,
    pub name: String,
    pub pages: i64,
    pub done: i64,
    pub updated: f64,
}

/// A page file that exists but could not be parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct Unreadable {
    pub id: String,
    pub error: String,
}

/// What `openProject` hands to the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub slug: String,
    pub assignment: Assignment,
    /// Pages whose file could not be parsed. Their ids stay in `meta.order`; the files are never written.
    pub unreadable: Vec<Unreadable>,
    /// `project.json` was damaged and the meta came from `project.json.bak`.
    pub from_backup: bool,
    /// ... and the backup was copied over `project.json` (`restore: true`).
    pub restored: bool,
}

impl Opened {
    /// `meta.ui.selectedId` when that page loaded, else the first page.
    pub fn selected_id(&self) -> Option<String> {
        let sel = self
            .assignment
            .meta
            .get("ui")
            .and_then(|u| u.get("selectedId"))
            .and_then(Value::as_str)
            .filter(|id| self.assignment.pages.contains_key(*id));
        sel.map(String::from).or_else(|| self.assignment.meta.order().into_iter().find(|id| self.assignment.pages.contains_key(id)))
    }
}

/// Result of an import (`legacy` or files).
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub slug: String,
    pub pages: usize,
    pub with_image: usize,
}

pub struct Projects<S: Store> {
    pub store: S,
}

impl<S: Store> Projects<S> {
    pub fn new(store: S) -> Self {
        Projects { store }
    }

    // ---------------------------------------------------------------- raw text / JSON

    /// `fs.read`: the file as text, `None` when missing.
    pub async fn read_text(&self, path: &str) -> Result<Option<String>> {
        Ok(self.store.get(path).await?.map(|b| decode_text(&b)))
    }

    /// `fs.write` for text.
    pub async fn write_text(&self, path: &str, text: &str) -> Result<()> {
        self.store.put(path, text.as_bytes()).await
    }

    /// `safeJson`: for `project.json` the previous copy goes to `.bak` first, but only while it still
    /// parses (a failed backup write is ignored, exactly as in JS). Then the new text is written.
    pub async fn safe_json(&self, path: &str, text: &str) -> Result<()> {
        if is_project_json(path) {
            if let Ok(Some(old)) = self.read_text(path).await {
                if !old.is_empty() && lenient::is_valid_json(&old) {
                    let _ = self.write_text(&format!("{path}.bak"), &old).await;
                }
            }
        }
        self.write_text(path, text).await
    }

    fn pretty(meta: &Project) -> Result<String> {
        meta.to_json_pretty().map_err(|e| StoreError::Invalid(e.to_string()))
    }

    // ---------------------------------------------------------------- project.json

    /// `readMeta`. `Ok(None)` when project.json is absent. `Err(Parse)` when neither project.json nor its
    /// backup is usable (nothing is written). With `restore`, a usable backup is copied over a damaged
    /// project.json (as JS does; no extra files). Valid JSON that cannot be represented is not damaged.
    async fn read_meta(&self, slug: &str, restore: bool, now: i64) -> Result<Option<(Project, bool, bool)>> {
        let path = format!("{slug}/project.json");
        let Some(txt) = self.read_text(&path).await? else { return Ok(None) };
        let first_err = match Project::from_json(&txt) {
            Ok(m) => return Ok(Some((m.normalized(now), false, false))),
            Err(e) => e,
        };
        // Valid JSON that this crate cannot represent (the JS app opens it): not damaged, never replaced.
        if lenient::is_valid_json(&txt) && lenient::parse::<Value>(&txt).is_err() {
            return Err(parse_err(&path, first_err));
        }
        if let Some(bak) = self.read_text(&format!("{path}.bak")).await? {
            if let Ok(m) = Project::from_json(&bak) {
                let mut restored = false;
                if restore {
                    self.write_text(&path, &bak).await?;
                    restored = true;
                }
                return Ok(Some((m.normalized(now), true, restored)));
            }
        }
        Err(parse_err(&path, first_err))
    }

    /// Meta of a project without loading its pages (`readMeta(slug, false)`).
    pub async fn meta(&self, slug: &str, now: i64) -> Result<Option<Project>> {
        Ok(self.read_meta(slug, false, now).await?.map(|m| m.0))
    }

    async fn unique_slug(&self, base: &str) -> Result<String> {
        // Case-insensitive: Windows / macOS folders alias `Quiz` and `quiz`.
        let taken: BTreeSet<String> = self.store.children("").await?.into_iter().map(|(n, _)| n.to_lowercase()).collect();
        let base = if base.is_empty() { "project" } else { base };
        let mut s = base.to_string();
        let mut i = 2;
        // a device name (`con`, `nul`, `com1`) is unusable as a folder on Windows: treat it as taken
        while taken.contains(&s.to_lowercase()) || crate::is_device_name(&s) {
            s = format!("{base}-{i}");
            i += 1;
        }
        Ok(s)
    }

    // ---------------------------------------------------------------- list / create / open

    /// `listProjects`: newest first. Projects with an unusable project.json are skipped.
    pub async fn list_projects(&self, now: i64) -> Result<Vec<Summary>> {
        let mut out = vec![];
        for (name, is_dir) in self.store.children("").await? {
            if !is_dir || name.starts_with('.') {
                continue;
            }
            let Ok(Some((m, _, _))) = self.read_meta(&name, false, now).await else { continue };
            let stat = |k: &str| -> Option<i64> {
                let v = m.get("stats").and_then(|s| s.get(k));
                if jsx::truthy(v) {
                    Some(jsx::number(v.unwrap()) as i64)
                } else {
                    None
                }
            };
            let updated = m.get("updated").map(jsx::number).filter(|n| !n.is_nan()).unwrap_or(0.0);
            out.push(Summary {
                slug: name,
                name: m.get("name").map(jsx::to_string).unwrap_or_default(),
                pages: stat("pages").unwrap_or(m.order_len().unwrap_or(0) as i64),
                done: stat("done").unwrap_or(0),
                updated,
            });
        }
        out.sort_by(|a, b| b.updated.partial_cmp(&a.updated).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    /// `createProject`: a unique slug, a fresh project.json (plain write, so no `.bak` yet).
    pub async fn create_project(&self, name: &str, now: i64) -> Result<String> {
        let slug = self.unique_slug(&make_slug(name)).await?;
        let mut m = Map::new();
        m.insert("name".into(), json!(name));
        m.insert("prompt".into(), json!(""));
        m.insert("created".into(), json!(now));
        m.insert("updated".into(), json!(now));
        m.insert("stats".into(), json!({"pages": 0, "done": 0}));
        let mut meta = Project::empty();
        for (k, v) in m {
            meta.set(&k, v);
        }
        let meta = meta.normalized(now);
        // never overwrite an existing project.json (aliased names, racing creators)
        if self.store.get(&format!("{slug}/project.json")).await?.is_some() {
            return Err(StoreError::Invalid(format!("{slug}/project.json already exists")));
        }
        self.write_text(&format!("{slug}/project.json"), &Self::pretty(&meta)?).await?;
        Ok(slug)
    }

    /// `openProject`: meta (normalised), pages, crash recovery (`transcribing` becomes `pending` in memory
    /// only). `Ok(None)` when the project does not exist.
    pub async fn open_project(&self, slug: &str, restore: bool, now: i64) -> Result<Option<Opened>> {
        let Some((mut meta, from_backup, restored)) = self.read_meta(slug, restore, now).await? else {
            return Ok(None);
        };
        let order = meta.order();
        let mut pages: BTreeMap<String, Page> = BTreeMap::new();
        let mut unreadable = vec![];
        let mut keep = vec![];
        for id in &order {
            let path = format!("{slug}/pages/{id}.json");
            match self.read_text(&path).await {
                Ok(Some(t)) if !t.is_empty() => match Page::from_json(&t) {
                    Ok(mut p) => {
                        p.1 = Some(id.clone());
                        pages.insert(id.clone(), p);
                    }
                    Err(e) => unreadable.push(Unreadable { id: id.clone(), error: e.to_string() }),
                },
                Ok(_) => continue, // missing or empty: dropped from the order, like JS
                Err(e) => unreadable.push(Unreadable { id: id.clone(), error: e.to_string() }),
            }
            keep.push(id.clone()); // an unreadable page that exists on disk is never dropped
        }
        if keep != order {
            meta.set_order(&keep);
        }
        for p in pages.values_mut() {
            if p.status() == "transcribing" {
                p.set("status", json!("pending"));
            }
        }
        let mut assignment = Assignment::new(meta);
        assignment.pages = pages;
        Ok(Some(Opened { slug: slug.into(), assignment, unreadable, from_backup, restored }))
    }

    // ---------------------------------------------------------------- saves

    /// `saveMeta`: with `touch = Some(now)` sets `meta.updated` first (the JS app always does); with `None`
    /// the document is written as is. project.json goes through `safe_json` (backup first).
    pub async fn save_meta(&self, slug: &str, meta: &mut Project, touch: Option<i64>) -> Result<()> {
        if let Some(now) = touch {
            meta.set("updated", json!(now));
        }
        self.safe_json(&format!("{slug}/project.json"), &Self::pretty(meta)?).await
    }

    /// `savePage`: compact JSON, plain write.
    pub async fn save_page(&self, slug: &str, page: &Page) -> Result<()> {
        let key = page.file_key();
        if key.is_empty() {
            return Err(StoreError::Invalid("page has no id and was not loaded from a project".into()));
        }
        let text = page.to_json().map_err(|e| StoreError::Invalid(e.to_string()))?;
        self.write_text(&format!("{slug}/pages/{key}.json"), &text).await
    }

    pub async fn save_image(&self, slug: &str, id: &str, bytes: &[u8]) -> Result<()> {
        self.store.put(&format!("{slug}/images/{id}.jpg"), bytes).await
    }

    pub async fn save_original(&self, slug: &str, id: &str, ext: &str, bytes: &[u8]) -> Result<()> {
        self.store.put(&format!("{slug}/originals/{id}.{ext}"), bytes).await
    }

    pub async fn load_image(&self, slug: &str, id: &str) -> Result<Option<Vec<u8>>> {
        self.store.get(&format!("{slug}/images/{id}.jpg")).await
    }

    /// `deletePage`: the page file, its image and every `originals/<id>.*`.
    pub async fn delete_page(&self, slug: &str, id: &str) -> Result<()> {
        self.store.delete_tree(&format!("{slug}/pages/{id}.json")).await?;
        self.store.delete_tree(&format!("{slug}/images/{id}.jpg")).await?;
        let pre = format!("{id}.");
        for (name, is_dir) in self.store.children(&format!("{slug}/originals")).await? {
            if !is_dir && name.starts_with(&pre) {
                self.store.delete_tree(&format!("{slug}/originals/{name}")).await?;
            }
        }
        Ok(())
    }

    /// `renameProject` for a project that is not open: plain write, no `.bak`. `false` when unusable.
    pub async fn rename_project(&self, slug: &str, name: &str, now: i64) -> Result<bool> {
        let Some((mut m, _, _)) = self.read_meta(slug, true, now).await? else { return Ok(false) };
        m.set_name(name);
        m.set("updated", json!(now));
        self.write_text(&format!("{slug}/project.json"), &Self::pretty(&m)?).await?;
        Ok(true)
    }

    async fn copy_tree(&self, src: &str, dst: &str) -> Result<()> {
        for k in self.store.list(&format!("{src}/")).await? {
            // a name the store lists but can never read (device names, ':' ...) is skipped
            let b = match self.store.get(&k).await {
                Ok(b) => b,
                Err(StoreError::InvalidKey(_)) => continue,
                Err(e) => return Err(e),
            };
            if let Some(b) = b {
                match self.store.put(&format!("{dst}/{}", &k[src.len() + 1..]), &b).await {
                    Err(StoreError::InvalidKey(_)) => {}
                    r => r?,
                }
            }
        }
        Ok(())
    }

    /// `duplicateProject`: copy everything, then rename the copy `<name> copy`. Returns the new slug.
    pub async fn duplicate_project(&self, slug: &str, now: i64) -> Result<Option<String>> {
        let Some((mut m, _, _)) = self.read_meta(slug, true, now).await? else { return Ok(None) };
        let dst = self.unique_slug(&format!("{slug}-copy")).await?;
        self.copy_tree(slug, &dst).await?;
        let name = format!("{} copy", m.get("name").map(jsx::to_string).unwrap_or_default());
        m.set_name(&name);
        m.set("created", json!(now));
        m.set("updated", json!(now));
        self.write_text(&format!("{dst}/project.json"), &Self::pretty(&m)?).await?;
        Ok(Some(dst))
    }

    /// `deleteProject`.
    pub async fn delete_project(&self, slug: &str) -> Result<()> {
        self.store.delete_tree(slug).await
    }

    /// `writeExport`.
    pub async fn write_export(&self, slug: &str, name: &str, bytes: &[u8]) -> Result<()> {
        self.store.put(&format!("{slug}/exports/{name}"), bytes).await
    }

    // ---------------------------------------------------------------- zip / import

    /// Files of a project for a project zip (everything except `exports/`), keyed by `<slug>/...` like
    /// `exportProjectZip`.
    pub async fn project_files(&self, slug: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let skip = format!("{slug}/exports/");
        let mut out = vec![];
        for k in self.store.list(&format!("{slug}/")).await? {
            if k.starts_with(&skip) {
                continue;
            }
            match self.store.get(&k).await {
                Ok(Some(b)) => out.push((k, b)),
                Ok(None) | Err(StoreError::InvalidKey(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// `importFiles` after the zip / folder was unpacked into `(path, bytes)` entries. Writes the project
    /// under a fresh slug; skips `exports/`.
    pub async fn import_files(&self, entries: &[(String, Vec<u8>)]) -> Result<String> {
        Ok(self.import_files_report(entries).await?.0)
    }

    /// Like [`import_files`] and also returns the entries the store could never hold (reported, not lost
    /// silently).
    pub async fn import_files_report(&self, entries: &[(String, Vec<u8>)]) -> Result<(String, Vec<String>)> {
        let mut skipped = vec![];
        let mut pj: Vec<&(String, Vec<u8>)> = entries.iter().filter(|(p, _)| is_project_json(p)).collect();
        pj.sort_by_key(|(p, _)| p.len());
        let Some((pj_path, pj_bytes)) = pj.first().map(|e| (&e.0, &e.1)) else {
            return Err(StoreError::Invalid("No project.json found. Pick a project folder or a project zip.".into()));
        };
        let meta = Project::from_json(&decode_text(pj_bytes))
            .map_err(|_| StoreError::Invalid("project.json in that selection is damaged.".into()))?;
        let base = &pj_path[..pj_path.len() - "project.json".len()];
        let nm = meta.get("name");
        let name = if jsx::truthy(nm) { jsx::to_string(nm.unwrap()) } else { "imported".into() };
        let base_slug = make_slug(&name);
        let slug = self.unique_slug(if base_slug.is_empty() { "imported" } else { &base_slug }).await?;
        for (path, bytes) in entries {
            let Some(rel) = path.strip_prefix(base) else { continue };
            if rel.is_empty() || rel.starts_with("exports/") {
                continue;
            }
            match self.store.put(&format!("{slug}/{rel}"), bytes).await {
                // a name the store can never hold (device names, trailing dot): skip it, keep importing
                Err(StoreError::InvalidKey(_)) => skipped.push(path.clone()),
                r => r?,
            }
        }
        Ok((slug, skipped))
    }

    /// Write a legacy import plan (`vishion_core::import::plan`) as `importLegacy` does: create the
    /// project, then image + page per session, then the final project.json.
    pub async fn apply_legacy(&self, plan: &Plan, now: i64) -> Result<Imported> {
        let slug = self.create_project(&plan.name, now).await?;
        for pp in &plan.pages {
            if let Some(img) = &pp.image {
                self.save_image(&slug, pp.page.id(), img).await?;
            }
            self.save_page(&slug, &pp.page).await?;
        }
        self.write_text(&format!("{slug}/project.json"), &Self::pretty(&plan.meta)?).await?;
        Ok(Imported { slug, pages: plan.pages.len(), with_image: plan.with_image })
    }

    // ---------------------------------------------------------------- settings.json

    /// `loadFolderSettings`: the parsed file with secrets stripped; `None` when absent. A damaged file is
    /// an error and is left alone.
    pub async fn load_settings(&self) -> Result<Option<Map<String, Value>>> {
        let Some(t) = self.read_text(SETTINGS_PATH).await? else { return Ok(None) };
        if t.is_empty() {
            return Ok(None);
        }
        match lenient::parse::<Value>(&t) {
            Ok(Value::Object(mut m)) => {
                for k in SETTINGS_STRIP {
                    m.remove(k);
                }
                Ok(Some(m))
            }
            Ok(_) => Ok(Some(Map::new())),
            Err(e) => Err(parse_err(SETTINGS_PATH, e)),
        }
    }

    /// `fs.saveSettings`: pretty, without the stripped keys, plain write.
    pub async fn save_settings(&self, s: &Settings) -> Result<()> {
        let mut m = s.0.clone();
        for k in SETTINGS_STRIP {
            m.remove(k);
        }
        let text = lenient::to_string(&Value::Object(m), true).map_err(|e| StoreError::Invalid(e.to_string()))?;
        self.write_text(SETTINGS_PATH, &text).await
    }
}
