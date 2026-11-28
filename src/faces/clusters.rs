//! The face pass and the grouping: the scan, the union-find components and the report.

use super::embed::{cosine, FaceModel};
use super::matches::{match_named, MatchReport};
use super::MODEL_ID;
use crate::db::{self, FaceRow, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub images: usize,
    pub todo: usize,
    /// Photos the pass did not take because their catalog has faces off (M26.9).
    pub pass_skipped: usize,
}

pub fn plan(db_path: &Path) -> Result<Plan> {
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    // A switched-off folder is not planned for (M26.1); a catalog with faces off is counted apart
    // and left alone (M26.9).
    let rows: Vec<db::FileRow> = db::load_all(&conn)?
        .into_iter()
        .filter(|r| !settings.path_hidden(&r.path))
        .collect();
    let pass_skipped = rows
        .iter()
        .filter(|r| r.kind == "image" && !settings.pass_allows(&r.path, db::Pass::Faces))
        .count();
    let rows: Vec<db::FileRow> = rows
        .into_iter()
        .filter(|r| settings.pass_allows(&r.path, db::Pass::Faces))
        .collect();
    let scans = db::face_scan_shas(&conn)?;
    let images = rows.iter().filter(|r| r.kind == "image").count();
    let todo = rows
        .iter()
        .filter(|r| r.kind == "image" && scans.get(&r.id) != Some(&r.sha256))
        .count();
    Ok(Plan {
        images,
        todo,
        pass_skipped,
    })
}

pub struct Stats {
    pub images: usize,
    pub done: usize,
    pub skipped: usize,
    /// Photos the pass did not take because their catalog has faces off (M26.9).
    pub pass_skipped: usize,
    pub faces: usize,
    pub clusters: usize,
    /// What the matching step did after the clustering (M26.11).
    pub matched: MatchReport,
    pub errors: usize,
    pub seconds: f64,
    pub cancelled: bool,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = i18n::t(
            self.lang,
            if self.cancelled {
                "stopped: "
            } else {
                ""
            },
        );
        let body = i18n::fill(
            self.lang,
            "scanned {} of {} photos, {} faces, {} clusters, {} errors, {} s",
            &[
                self.done.to_string(),
                self.images.to_string(),
                self.faces.to_string(),
                self.clusters.to_string(),
                self.errors.to_string(),
                format!("{:.1}", self.seconds),
            ],
        );
        write!(f, "{prefix}{body}")?;
        if self.skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped, already scanned)",
                &[self.skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped — their folders have faces off)",
                &[self.pass_skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.matched.worth_telling() {
            let note = format!("; {}", self.matched);
            write!(f, "{note}")?;
        }
        Ok(())
    }
}

/// A pass over the photos without faces in the DB, then clustering. Videos do not take part.
pub fn run(
    db_path: &Path,
    jobs: usize,
    threshold: f32,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    // A switched-off folder takes no part in the pass (M26.1): its files are hidden, and the
    // owner switches it back on to let the pass catch up.
    let rows: Vec<db::FileRow> = db::load_all(&conn)?
        .into_iter()
        .filter(|r| !settings.path_hidden(&r.path))
        .collect();
    // A catalog with faces off is counted apart and left alone (M26.9).
    let pass_skipped = rows
        .iter()
        .filter(|r| r.kind == "image" && !settings.pass_allows(&r.path, db::Pass::Faces))
        .count();
    let rows: Vec<db::FileRow> = rows
        .into_iter()
        .filter(|r| settings.pass_allows(&r.path, db::Pass::Faces))
        .collect();
    let scans = db::face_scan_shas(&conn)?;
    let images = rows.iter().filter(|r| r.kind == "image").count();
    let todo: Vec<db::FileRow> = rows
        .into_iter()
        .filter(|r| r.kind == "image" && scans.get(&r.id) != Some(&r.sha256))
        .collect();
    let skipped = images - todo.len();

    if progress.as_ref().is_some_and(|p| p.cancelled()) {
        if let Some(p) = &progress {
            p.finish();
        }
        return Ok(Stats {
            images,
            done: 0,
            skipped,
            pass_skipped,
            faces: 0,
            clusters: 0,
            matched: MatchReport {
                lang: settings.lang,
                ..MatchReport::default()
            },
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: true,
            lang: settings.lang,
        });
    }
    if todo.is_empty() {
        let (faces, _, clusters) = db::face_summary(&conn)?;
        return Ok(Stats {
            images,
            done: 0,
            skipped,
            pass_skipped,
            faces: faces as usize,
            clusters: clusters as usize,
            matched: MatchReport {
                lang: settings.lang,
                ..MatchReport::default()
            },
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: false,
            lang: settings.lang,
        });
    }

    let model = Arc::new(Mutex::new(FaceModel::load(&settings, jobs)?));
    let faces_dir = db::cache_dir().join("faces");
    std::fs::create_dir_all(&faces_dir)?;
    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg(i18n::fill(
            settings.lang,
            "faces: model {}, threads {}",
            &[MODEL_ID.to_string(), jobs.to_string()],
        ));
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()?;
    let (tx, rx) = mpsc::channel::<(i64, String, Vec<FaceRow>)>();
    let writer = {
        let db_path = db_path.to_path_buf();
        std::thread::spawn(move || -> Result<usize> {
            let mut conn = db::open(&db_path)?;
            let tx = conn.transaction()?;
            let mut n = 0usize;
            for (file_id, sha, faces) in rx {
                db::save_faces_for_file(&tx, file_id, &sha, &faces)?;
                n += 1;
            }
            tx.commit()?;
            Ok(n)
        })
    };
    let errors = AtomicUsize::new(0);
    pool.install(|| {
        todo.par_iter().for_each_with(tx, |tx, f| {
            if progress.as_ref().is_some_and(|p| p.cancelled()) {
                return;
            }
            match scan_one(&model, &faces_dir, f) {
                Ok(faces) => {
                    let _ = tx.send((f.id, f.sha256.clone(), faces));
                }
                Err(e) => {
                    errors.fetch_add(1, Ordering::Relaxed);
                    eprintln!("\n{}", i18n::fill(settings.lang, "faces {}: {}", &[f.path.clone(), format!("{e:#}")]));
                    if let Some(p) = &progress {
                        p.error();
                    }
                    // A broken or unreadable photo is a result too: we do not come back to it every time.
                    // The counter shows the error, and its face count is zero.
                    let _ = tx.send((f.id, f.sha256.clone(), Vec::new()));
                }
            }
            if let Some(p) = &progress {
                p.tick();
            }
        })
    });
    let done = writer
        .join()
        .map_err(|_| anyhow::anyhow!(i18n::t(settings.lang, "the face write thread died")))??;
    let cancelled = progress.as_ref().is_some_and(|p| p.cancelled());
    if let Some(p) = &progress {
        p.set_msg(i18n::t(
            settings.lang,
            if cancelled {
                "faces: stopped"
            } else {
                "faces: clusters"
            },
        ));
    }
    let (clusters, matched) = if cancelled {
        (
            db::face_summary(&conn).map(|(_, _, c)| c as usize).unwrap_or(0),
            MatchReport {
                lang: settings.lang,
                ..MatchReport::default()
            },
        )
    } else {
        // The rebuild ends with the matching step (M26.11); its numbers are part of the report.
        let cs = recluster(db_path, threshold)?;
        (cs.clusters, cs.matched)
    };
    let faces: i64 = db::open(db_path)?.query_row("SELECT COUNT(*) FROM faces", [], |r| r.get(0))?;
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "done"));
        p.finish();
    }
    Ok(Stats {
        images,
        done,
        skipped,
        pass_skipped,
        faces: faces as usize,
        clusters,
        matched,
        errors: errors.load(Ordering::Relaxed),
        seconds: t0.elapsed().as_secs_f64(),
        cancelled,
        lang: settings.lang,
    })
}

fn scan_one(
    model: &Mutex<FaceModel>,
    faces_dir: &Path,
    f: &db::FileRow,
) -> Result<Vec<FaceRow>> {
    let img = derive::load_oriented(Path::new(&f.path))?;
    let detected = {
        let mut m = model
            .lock()
            .map_err(|_| anyhow::anyhow!("the face model is busy and does not answer"))?;
        m.analyze(&img)?
    };
    let mut rows = Vec::with_capacity(detected.len());
    let sha = &f.sha256[..24.min(f.sha256.len())];
    for (i, d) in detected.iter().enumerate() {
        let rel = format!("{}/{sha}-{i}.jpg", &f.sha256[..2.min(f.sha256.len())]);
        let path = faces_dir.join(&rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        d.aligned
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .with_context(|| format!("writing the face {}", path.display()))?;
        rows.push(FaceRow {
            id: 0,
            x: d.x,
            y: d.y,
            w: d.w,
            h: d.h,
            score: d.score,
            quality: d.quality,
            thumb: Some(rel),
            embedding: d.embedding.clone(),
            cluster_id: None,
            manual: false,
        });
    }
    Ok(rows)
}

pub struct ClusterStats {
    pub faces: usize,
    pub clusters: usize,
    pub fixed: usize,
    /// Faces the rebuild left alone because their catalog has faces off (M26.9).
    pub pass_skipped: usize,
    /// What the matching step did after the rebuild (M26.11).
    pub matched: MatchReport,
    pub seconds: f64,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for ClusterStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = i18n::fill(
            self.lang,
            "{} faces, {} groups, {} pinned by hand, {} s",
            &[
                self.faces.to_string(),
                self.clusters.to_string(),
                self.fixed.to_string(),
                format!("{:.1}", self.seconds),
            ],
        );
        write!(f, "{text}")?;
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} faces of catalogs with faces off kept as they are)",
                &[self.pass_skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.matched.worth_telling() {
            let note = format!("; {}", self.matched);
            write!(f, "{note}")?;
        }
        Ok(())
    }
}

/// Rebuild the groups from the embeddings. Named clusters and manual edits are kept.
pub fn recluster(db_path: &Path, threshold: f32) -> Result<ClusterStats> {
    let t0 = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;
    let faces = db::load_faces(&conn)?;
    if faces.is_empty() {
        return Ok(ClusterStats {
            faces: 0,
            clusters: 0,
            fixed: 0,
            pass_skipped: 0,
            matched: MatchReport {
                lang,
                ..MatchReport::default()
            },
            seconds: 0.0,
            lang,
        });
    }
    let clusters = db::load_face_clusters(&conn)?;
    let mut fixed: HashSet<i64> = HashSet::new();
    for c in &clusters {
        if !c.label.trim().is_empty() {
            fixed.insert(c.id);
        }
    }
    for f in &faces {
        if f.manual && let Some(cid) = f.cluster_id {
            fixed.insert(cid);
        }
    }
    // A catalog with faces off keeps the groups it has: its faces are not re-clustered (M26.9).
    let paths = db::face_paths(&conn)?;
    let allowed: HashSet<i64> = paths
        .iter()
        .filter(|(_, path)| settings.pass_allows(path, db::Pass::Faces))
        .map(|(id, _)| *id)
        .collect();
    let pass_skipped = faces.iter().filter(|f| !allowed.contains(&f.id)).count();
    let free: Vec<&FaceRow> = faces
        .iter()
        .filter(|f| allowed.contains(&f.id))
        .filter(|f| f.cluster_id.is_none_or(|c| !fixed.contains(&c)))
        .collect();
    if !free.is_empty() {
        let vectors: Vec<&[f32]> = free.iter().map(|f| f.embedding.as_slice()).collect();
        let comps = components(&vectors, threshold);
        let mut groups: HashMap<usize, Vec<i64>> = HashMap::new();
        for (i, root) in comps.into_iter().enumerate() {
            groups.entry(root).or_default().push(free[i].id);
        }
        let mut assignments = Vec::with_capacity(free.len());
        for members in groups.values() {
            let cid = db::create_face_cluster(&conn)?;
            for &fid in members {
                assignments.push((fid, Some(cid)));
            }
        }
        db::save_face_assignments(&mut conn, &assignments, false)?;
        db::delete_empty_face_clusters(&conn)?;
    }
    let total = db::load_face_clusters(&conn)?.len();
    // The named people are matched right after the rebuild (M26.11): a face computed in another
    // catalog joins the person it resembles, so the owner does not name the same grandmother once
    // per folder. The connection is closed first — the step writes through its own.
    drop(conn);
    let matched = match_named(db_path, &settings)?;
    Ok(ClusterStats {
        faces: faces.len(),
        clusters: total,
        fixed: fixed.len(),
        pass_skipped,
        matched,
        seconds: t0.elapsed().as_secs_f64(),
        lang,
    })
}

/// The components of the "cosine ≥ threshold" graph. Returns the root of every vector.
pub(super) fn components(vectors: &[&[f32]], threshold: f32) -> Vec<usize> {
    let n = vectors.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for i in 0..n {
        for j in i + 1..n {
            if cosine(vectors[i], vectors[j]) >= threshold {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    (0..n).map(|i| find(&mut parent, i)).collect()
}
