//! The automatic matches: a proposal waits in the quarantine until the owner confirms it.

use super::embed::cosine;
use crate::db;
use crate::i18n::{self, Lang};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn label(db_path: &Path, cluster_id: i64, label: &str) -> Result<MatchReport> {
    let conn = db::open(db_path)?;
    db::label_face_cluster(&conn, cluster_id, label)?;
    drop(conn);
    // The name is the last decision (M26.11): the matching step runs at once, so the owner does
    // not have to rebuild the groups to see the person recognized in another catalog.
    match_named(db_path, &{
        let conn = db::open(db_path)?;
        db::load_settings(&conn)
    })
}

/// The result of one matching step (M26.11, proposals since M26.17): how many free faces were
/// proposed for a named person, how many people received one, and why the rest stayed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MatchReport {
    pub matched: usize,
    /// The named groups that received at least one face.
    pub people: usize,
    /// The named people that were there before the step.
    pub named: usize,
    /// Faces left because the best named person did not lead the second one by the margin.
    pub left_small: usize,
    /// Faces left because no named person was close enough.
    pub left_far: usize,
    pub seconds: f64,
    /// A plan (`--dry-run`): the same numbers, nothing written.
    pub dry_run: bool,
    /// The language of the report (M21.6): English by default.
    pub lang: Lang,
}

impl MatchReport {
    /// Whether there is something to tell: a report about a database without a single named person
    /// is noise in a pass report, so the pass leaves it out.
    pub fn worth_telling(&self) -> bool {
        self.named > 0
    }
}

impl std::fmt::Display for MatchReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.named == 0 {
            return f.write_str(i18n::t(
                self.lang,
                "no named people yet: there is nothing to match to",
            ));
        }
        // "proposed 0 faces to 0 named people" would read as if nobody were named at all, so a
        // pass that proposed nothing says how many people were there to match against.
        let head = if self.matched == 0 {
            i18n::fill(
                self.lang,
                "no new proposal yet: {} named",
                &[self.named.to_string()],
            )
        } else if self.dry_run {
            i18n::fill(
                self.lang,
                "would propose {} faces to {} named people",
                &[self.matched.to_string(), self.people.to_string()],
            )
        } else {
            i18n::fill(
                self.lang,
                "proposed {} faces to {} named people — they wait in quarantine",
                &[self.matched.to_string(), self.people.to_string()],
            )
        };
        write!(f, "{head}")?;
        if self.left_small > 0 {
            write!(
                f,
                "; {}",
                i18n::fill(
                    self.lang,
                    "{} left — the lead was too small",
                    &[self.left_small.to_string()],
                )
            )?;
        }
        if self.left_far > 0 {
            write!(
                f,
                "; {}",
                i18n::fill(
                    self.lang,
                    "{} left — no named person is close enough",
                    &[self.left_far.to_string()],
                )
            )?;
        }
        Ok(())
    }
}

/// A free face that resembles a named person is proposed for that person (M26.11, quarantine
/// since M26.17): the owner names a grandmother once and every catalog offers her faces for
/// confirmation. Nothing joins a group by itself. Only free faces are proposed — a face with
/// `manual = 1` (named, gathered or split off by hand) and a face that already sits in a named
/// group never are. The best named group wins only when it is at least `settings.face_match` close
/// **and** leads the second named group by `settings.face_match_margin`; a relative's face is left
/// alone rather than guessed. The step replaces the proposal table and touches no membership, no
/// crop and nothing on disk.
pub fn match_named(db_path: &Path, settings: &db::Settings) -> Result<MatchReport> {
    match_named_in(db_path, settings, false)
}

/// The plan of the matching step: the same numbers, not a single row written (`faces --dry-run`).
pub fn plan_named(db_path: &Path, settings: &db::Settings) -> Result<MatchReport> {
    match_named_in(db_path, settings, true)
}

fn match_named_in(
    db_path: &Path,
    settings: &db::Settings,
    dry_run: bool,
) -> Result<MatchReport> {
    let t0 = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let faces = db::load_faces(&conn)?;
    let clusters = db::load_face_clusters(&conn)?;
    // The named groups: their members are the reference vectors of every person.
    let named: HashSet<i64> = clusters
        .iter()
        .filter(|c| !c.label.trim().is_empty())
        .map(|c| c.id)
        .collect();
    let mut report = MatchReport {
        named: named.len(),
        dry_run,
        lang: settings.lang,
        ..MatchReport::default()
    };
    if named.is_empty() {
        // Nothing to match to: a proposal to a person without a name would be noise.
        if !dry_run {
            db::clear_face_matches(&conn)?;
        }
        report.seconds = t0.elapsed().as_secs_f64();
        return Ok(report);
    }
    let paths = db::face_paths(&conn)?;
    // The reference: group id → the vectors of its members. A face of a named group is a
    // reference and never a candidate; every other free face is compared with all of them.
    let mut targets: HashMap<i64, Vec<&[f32]>> = HashMap::new();
    let mut candidates: Vec<(i64, &[f32])> = Vec::new();
    for f in &faces {
        if let Some(cid) = f.cluster_id
            && named.contains(&cid)
        {
            targets.entry(cid).or_default().push(f.embedding.as_slice());
            continue;
        }
        // A face is free when the owner did not place it and it is not in a named group. A
        // catalog with the faces pass off takes no part (M26.9), and neither does a switched-off
        // folder (M26.1) — `pass_allows` says both.
        if f.manual {
            continue;
        }
        let allowed = paths
            .get(&f.id)
            .is_some_and(|p| settings.pass_allows(p, db::Pass::Faces));
        if !allowed {
            continue;
        }
        candidates.push((f.id, f.embedding.as_slice()));
    }
    // 2 500 faces × the named members is one pass of a few hundred milliseconds on one thread, so
    // the comparison runs in parallel; every face decides on its own and the result goes into one
    // transaction afterwards (M26.11).
    let scored: Vec<(i64, i64, f32, f32)> = candidates
        .par_iter()
        .map(|&(id, embedding)| {
            let (best, best_id, second) = best_two_groups(embedding, &targets);
            (id, best_id, best, second)
        })
        .collect();
    let mut hits: Vec<(i64, i64, f32)> = Vec::new();
    for (id, best_id, best, second) in scored {
        if best < settings.face_match {
            report.left_far += 1;
        } else if best - second < settings.face_match_margin {
            report.left_small += 1;
        } else {
            report.matched += 1;
            hits.push((id, best_id, best));
        }
    }
    report.people = hits
        .iter()
        .map(|(_, cid, _)| *cid)
        .collect::<HashSet<_>>()
        .len();
    if !dry_run {
        db::replace_face_matches(&mut conn, &hits)?;
    }
    report.seconds = t0.elapsed().as_secs_f64();
    Ok(report)
}

/// The closest named group, its similarity and the best similarity of any other named group.
pub(super) fn best_two_groups(embedding: &[f32], targets: &HashMap<i64, Vec<&[f32]>>) -> (f32, i64, f32) {
    let mut best = f32::MIN;
    let mut best_id = 0i64;
    let mut second = f32::MIN;
    for (&cid, members) in targets {
        let mut score = f32::MIN;
        for m in members {
            let c = cosine(embedding, m);
            if c > score {
                score = c;
            }
        }
        if score > best {
            second = best;
            best = score;
            best_id = cid;
        } else if score > second {
            second = score;
        }
    }
    (
        best,
        best_id,
        if second == f32::MIN { f32::MIN } else { second },
    )
}

/// The owner confirmed proposals (M26.17): the faces join the named people they resemble and are
/// pinned by hand. Rows only — no face crop and no photo is touched.
pub fn confirm_matches(db_path: &Path, face_ids: &[i64]) -> Result<usize> {
    let mut conn = db::open(db_path)?;
    db::confirm_face_matches(&mut conn, face_ids)
}

/// The owner turned proposals down (M26.17): the faces stay where they are and are pinned, so the
/// next step does not offer them again. Rows only.
pub fn reject_matches(db_path: &Path, face_ids: &[i64]) -> Result<usize> {
    let mut conn = db::open(db_path)?;
    db::reject_face_matches(&mut conn, face_ids)
}

/// The status of an answer to the quarantine (M26.17), with the right number: one face joined or
/// stayed, several joined or stayed. The app and the CLI say the same words.
pub fn answer_report(lang: Lang, moved: usize, confirm: bool) -> String {
    let (one, many) = if confirm {
        (
            "1 face joined the person it resembles",
            "{} faces joined the person they resemble",
        )
    } else {
        (
            "1 face stays where it is — the person is not them",
            "{} faces stay where they are — the person is not them",
        )
    };
    if moved == 1 {
        i18n::t(lang, one).to_string()
    } else {
        i18n::fill(lang, many, &[moved.to_string()])
    }
}

/// A batch assignment: the picked faces go into one group with one name.
pub fn group_picked(db_path: &Path, face_ids: &[i64], label: &str) -> Result<i64> {
    let conn = db::open(db_path)?;
    db::group_faces(&conn, face_ids, label)
}

/// Split a false merge: one face moves into a new group.
pub fn split(db_path: &Path, face_id: i64) -> Result<i64> {
    let mut conn = db::open(db_path)?;
    let cid = db::split_face(&mut conn, face_id)?;
    db::delete_empty_face_clusters(&conn)?;
    Ok(cid)
}

/// Take several picked faces out of their group in one action (M26.10): each gets an unnamed
/// group of its own, the rest stay, and the empty groups are cleared once. The caller keeps the
/// open group: this function never jumps the screen anywhere.
pub fn split_many(db_path: &Path, face_ids: &[i64]) -> Result<db::SplitOutcome> {
    let mut conn = db::open(db_path)?;
    let out = db::split_faces(&mut conn, face_ids)?;
    db::delete_empty_face_clusters(&conn)?;
    Ok(out)
}
