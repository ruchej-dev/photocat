//! Downloading the weights: consent, size limit, sha256 and the progress line.

use super::model::{active_entry, catalog, entry, file_paths, model_dir, ModelEntry, ModelFile};
use crate::i18n::Lang;
use crate::db::{self, Progress};
use crate::derive;
use crate::i18n;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::Arc;

/// The model files are in place (without a sha256 check — that happens on download).
pub fn model_present_in(models_dir: &Path, e: &ModelEntry) -> bool {
    let dir = model_dir(models_dir, e);
    file_paths(&dir, e).iter().all(|p| p.is_file())
}

pub fn model_present(settings: &db::Settings) -> bool {
    model_present_in(&settings.models_dir, active_entry(settings))
}

/// Check the presence and the sha256 of every model file.
pub fn check_model_in(models_dir: &Path, e: &ModelEntry) -> Result<()> {
    let dir = model_dir(models_dir, e);
    for f in e.files {
        let path = dir.join(f.name);
        if !path.is_file() {
            bail!(
                "no files of the model “{}” in {}\ndownload it in the Settings or with `photocat models download --id {}`",
                e.title,
                dir.display(),
                e.id
            );
        }
        let got = derive::sha256_file(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if got != f.sha256 {
            bail!(
                "a model file is damaged ({}): {} \nsha256 {got}, expected {}\ndownload it again in the Settings",
                f.name,
                path.display(),
                f.sha256
            );
        }
    }
    Ok(())
}

/// The size of the model files in bytes (for the interface and the reports).
pub fn model_bytes_in(models_dir: &Path, e: &ModelEntry) -> i64 {
    let dir = model_dir(models_dir, e);
    file_paths(&dir, e)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as i64)
        .sum()
}

pub fn check_model(settings: &db::Settings) -> Result<()> {
    check_model_in(&settings.models_dir, active_entry(settings))
}

/// The result of a download: which model ended up downloaded and which options did not fit.
pub struct DownloadReport {
    pub model: &'static str,
    pub bytes: u64,
    pub files: usize,
    pub fallbacks: Vec<String>,
    /// The language of the report (M21.6): English by default.
    pub lang: Lang,
}

impl std::fmt::Display for DownloadReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut text = i18n::fill(
            self.lang,
            "model {}: {} files, {}",
            &[
                self.model.to_string(),
                self.files.to_string(),
                db::human_in(self.lang, self.bytes as i64),
            ],
        );
        if !self.fallbacks.is_empty() {
            text.push_str(&i18n::fill(
                self.lang,
                " (did not fit: {})",
                &[self.fallbacks.join("; ")],
            ));
        }
        f.write_str(&text)
    }
}

/// Download a model with the owner's consent. The chosen one first, then the compatible
/// options from the catalogue within the size limit (ADR 0015).
pub fn download_model(
    settings: &db::Settings,
    requested: &str,
    consent: bool,
    progress: Option<Arc<Progress>>,
) -> Result<DownloadReport> {
    if !consent {
        bail!(
            "{}",
            i18n::t(settings.lang, "downloading a model needs the owner's explicit consent")
        );
    }
    let max_bytes = settings.max_model_mb.saturating_mul(1024 * 1024);
    let mut candidates: Vec<&'static ModelEntry> = Vec::new();
    if let Some(e) = entry(requested) {
        candidates.push(e);
    }
    let mut rest: Vec<&'static ModelEntry> = catalog()
        .iter()
        .filter(|e| candidates.iter().all(|c| c.id != e.id))
        .collect();
    rest.sort_by_key(|e| e.bytes());
    candidates.extend(rest);

    let mut failed = Vec::new();
    for e in candidates {
        if max_bytes > 0 && e.bytes() > max_bytes {
            failed.push(format!(
                "{}: {} is bigger than the limit of {} MiB",
                e.title,
                db::human(e.bytes() as i64),
                settings.max_model_mb
            ));
            continue;
        }
        match download_entry(settings, e, progress.clone()) {
            Ok(files) => {
                return Ok(DownloadReport {
                    model: e.id,
                    bytes: e.bytes(),
                    files,
                    fallbacks: failed,
                    lang: settings.lang,
                });
            }
            Err(err) => failed.push(format!("{} — {err}", e.title)),
        }
    }
    bail!("cannot download the model: {}", failed.join("; "))
}

fn download_entry(
    settings: &db::Settings,
    e: &'static ModelEntry,
    progress: Option<Arc<Progress>>,
) -> Result<usize> {
    let dir = model_dir(&settings.models_dir, e);
    download_files(settings, &dir, e.title, e.files, progress)
}

/// Download an arbitrary set of model files (CLIP or faces) with a sha256 check.
/// A shared path so that there is one downloader (ADR 0047): the HTTP client is used only here.
pub(crate) fn download_files(
    settings: &db::Settings,
    dir: &Path,
    title: &str,
    model_files: &[ModelFile],
    progress: Option<Arc<Progress>>,
) -> Result<usize> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create {}", dir.display()))?;
    if let Some(p) = &progress {
        p.reset();
        p.set_total(model_files.len());
        p.set_msg(i18n::fill(settings.lang, "model {}: preparing", &[title.to_string()]));
    }
    let mut files = 0usize;
    for f in model_files {
        let path = dir.join(f.name);
        let ok = path.is_file()
            && derive::sha256_file(&path)
                .map(|got| got == f.sha256)
                .unwrap_or(false);
        if ok {
            files += 1;
        } else {
            if let Some(p) = &progress {
                p.set_msg(i18n::fill(
                    settings.lang,
                    "model {}: {}",
                    &[title.to_string(), f.name.to_string()],
                ));
            }
            fetch_with_fallback(f.url, &path, progress.as_deref(), title, f.name)?;
            let got = derive::sha256_file(&path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            if got != f.sha256 {
                bail!(
                    "the sha256 of {} does not match: got {got}, expected {}",
                    f.name,
                    f.sha256
                );
            }
            files += 1;
        }
        if let Some(p) = &progress {
            p.tick();
        }
    }
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "the model is downloaded"));
        p.finish();
    }
    Ok(files)
}

/// The main address and the mirror: if HuggingFace is unreachable, try hf-mirror.
fn fetch_with_fallback(
    url: &str,
    dst: &Path,
    progress: Option<&Progress>,
    title: &str,
    name: &str,
) -> Result<()> {
    let mirror = url.replace("https://huggingface.co/", "https://hf-mirror.com/");
    let mut last = String::new();
    for u in [url, mirror.as_str()] {
        if u.is_empty() {
            continue;
        }
        match http_get(u, dst, progress, title, name) {
            Ok(()) => return Ok(()),
            Err(e) => last = format!("{u}: {e}"),
        }
    }
    bail!("{last}");
}

/// The HTTP client of the weight downloader: one agent for the whole run, a connect timeout and a
/// total one, redirects followed (HuggingFace hands out CDN links). Timeouts on both sides keep a
/// stalled transfer from freezing the progress row forever.
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .timeout_global(Some(std::time::Duration::from_secs(1800)))
        .user_agent(concat!("photocat/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// One GET into `dst`, three attempts, the body streamed straight to the file (a model is 200–400
/// MiB, it must not sit in memory). The only network call of the application (ADR 0047): a weight
/// file from the catalogue, never anything the owner has not asked for.
fn http_get(
    url: &str,
    dst: &Path,
    progress: Option<&Progress>,
    title: &str,
    name: &str,
) -> Result<()> {
    let agent = http_agent();
    let mut last = String::new();
    for attempt in 1..=3 {
        match copy_body(&agent, url, dst, progress, title, name) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = format!("attempt {attempt}: {e}");
                // A stopped transfer is not retried: "Stop" is the owner's answer, not a flaky network.
                if progress.is_some_and(|p| p.cancelled()) {
                    break;
                }
                // A short pause and a fresh attempt: the retry is for a flaky network, not for a 404.
                std::thread::sleep(std::time::Duration::from_secs(attempt));
            }
        }
    }
    bail!("{last}");
}

fn copy_body(
    agent: &ureq::Agent,
    url: &str,
    dst: &Path,
    progress: Option<&Progress>,
    title: &str,
    name: &str,
) -> Result<()> {
    let mut resp = agent
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let total = resp.body().content_length();
    let mut body = resp.body_mut().as_reader();
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(dst).with_context(|| format!("cannot create {}", dst.display()))?,
    );
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut shown: u64 = 0;
    loop {
        // "Stop" (M26.5): the model download is the one network transfer the owner can interrupt.
        // The partial file fails the sha256 check next time, so nothing half-written is trusted.
        if progress.is_some_and(|p| p.cancelled()) {
            bail!("the download was stopped");
        }
        let n = std::io::Read::read(&mut body, &mut buf)
            .with_context(|| format!("reading {url}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .with_context(|| format!("writing {}", dst.display()))?;
        done += n as u64;
        // A line per 16 MiB: the progress row says how far a 400 MiB model has got.
        if let Some(p) = progress
            && done - shown >= 16 * 1024 * 1024
        {
            shown = done;
            let msg = match total {
                Some(t) => format!(
                    "{title}: {name} — {} of {}",
                    db::human(done as i64),
                    db::human(t as i64)
                ),
                None => format!("{title}: {name} — {}", db::human(done as i64)),
            };
            p.set_msg(msg);
        }
    }
    std::io::Write::flush(&mut file).context("flushing the downloaded model")?;
    if let Some(t) = total
        && done != t
    {
        bail!("the transfer ended early: {done} of {t} bytes");
    }
    Ok(())
}
