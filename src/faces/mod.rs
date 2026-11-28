//! Faces: SCRFD detection, alignment by 5 points, ArcFace embeddings and clusters.
//!
//! The models live in `<models_dir>/faces/`, are downloaded on consent only and checked by sha256.
//! No face, no embedding and no path ever leaves the machine (ADR 0011, 0021).
//!
//! Split by role (M27.5): `detect.rs` is the SCRFD pass, `align.rs` the five-point crop,
//! `embed.rs` the ArcFace session, `clusters.rs` the grouping pass and `matches.rs` the automatic
//! matches that wait for the owner. This module holds the model files and re-exports the public
//! names unchanged.

mod align;
mod clusters;
mod detect;
mod embed;
mod matches;

#[cfg(test)]
mod tests;

#[allow(unused_imports)] // the public surface stays whole even where the binary does not name it
pub use align::*;
pub use clusters::*;
pub use detect::*;
#[allow(unused_imports)] // the public surface stays whole even where the binary does not name it
pub use embed::*;
pub use matches::*;

use crate::db::{self, Progress};
use crate::derive;
use crate::embed::ModelFile;
use crate::i18n::{self, Lang};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

pub const MODEL_ID: &str = "buffalo_l";

pub const MODEL_DIR: &str = "faces";

const FILES: [ModelFile; 2] = [
    ModelFile {
        name: "det.onnx",
        url: "https://huggingface.co/immich-app/buffalo_l/resolve/main/detection/model.onnx",
        sha256: "5838f7fe053675b1c7a08b633df49e7af5495cee0493c7dcf6697200b85b5b91",
        bytes: 16_923_827,
    },
    ModelFile {
        name: "rec.onnx",
        url: "https://huggingface.co/immich-app/buffalo_l/resolve/main/recognition/model.onnx",
        sha256: "4c06341c33c2ca1f86781dab0e829f88ad5b64be9fba56e56bc9ebdefc619e43",
        bytes: 174_383_860,
    },
];

pub fn model_dir(settings: &db::Settings) -> PathBuf {
    settings.models_dir.join(MODEL_DIR)
}

pub fn model_present(settings: &db::Settings) -> bool {
    FILES.iter().all(|f| model_dir(settings).join(f.name).is_file())
}

pub fn model_bytes(settings: &db::Settings) -> i64 {
    FILES
        .iter()
        .filter_map(|f| std::fs::metadata(model_dir(settings).join(f.name)).ok())
        .map(|m| m.len() as i64)
        .sum()
}

pub fn check_model(settings: &db::Settings) -> Result<()> {
    for f in &FILES {
        let path = model_dir(settings).join(f.name);
        if !path.is_file() {
            bail!(
                "the face model files are missing: {}\ndownload them in the settings (with consent) or: \
                 `photocat faces download --yes`",
                path.display()
            );
        }
        let got = derive::sha256_file(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if got != f.sha256 {
            bail!(
                "the face model file is damaged ({}): {}\nsha256 {got}, expected {}",
                f.name,
                path.display(),
                f.sha256
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DownloadReport {
    pub files: usize,
    /// The language of the report (M21.6): English by default.
    pub lang: Lang,
}

impl std::fmt::Display for DownloadReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = i18n::fill(
            self.lang,
            "face model downloaded: {} files, {}",
            &[
                self.files.to_string(),
                db::human_in(self.lang, FILES.iter().map(|x| x.bytes as i64).sum()),
            ],
        );
        f.write_str(&text)
    }
}

/// Downloading the face weights the same way as CLIP: consent only, the built-in HTTP client, sha256.
pub fn download_model(
    settings: &db::Settings,
    consent: bool,
    progress: Option<Arc<Progress>>,
) -> Result<DownloadReport> {
    if !consent {
        bail!("{}", i18n::t(settings.lang, "downloading the face weights needs an explicit consent (the checkbox in the settings or --yes)"));
    }
    let bytes: u64 = FILES.iter().map(|f| f.bytes).sum();
    if bytes > settings.max_model_mb.saturating_mul(1024 * 1024) {
        bail!(
            "{}",
            i18n::fill(
                settings.lang,
                "the face model {} is bigger than the limit of {} MiB — raise the limit in the settings",
                &[db::human(bytes as i64), settings.max_model_mb.to_string()]
            )
        );
    }
    let files = crate::embed::download_files(
        settings,
        &model_dir(settings),
        "faces buffalo_l",
        &FILES,
        progress,
    )?;
    Ok(DownloadReport {
        files,
        lang: settings.lang,
    })
}
