//! The file rows, the index statistics and the rows that go with a file.

use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;

use crate::i18n::{self, Lang};

use super::albums::resequence_album_files;
use super::faces::delete_empty_face_clusters;
use super::human::{duration, human_in};
use super::schema::open;
use super::settings::{load_settings, under_root};
use super::tags::ocr_summary;

/// How many indexed files lie in each root, in the order of `roots`: the folder list says where
/// the archive is. A switched-off root is counted too — the switch hides, it does not delete.
pub fn root_file_counts(db: &Path, roots: &[String]) -> Result<Vec<(String, usize)>> {
    let conn = open(db)?;
    let paths: Vec<String> = {
        let mut st = conn.prepare("SELECT path FROM files WHERE trashed_at IS NULL")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()?
    };
    Ok(roots
        .iter()
        .map(|root| {
            let n = paths.iter().filter(|p| under_root(p, root)).count();
            (root.clone(), n)
        })
        .collect())
}

/// The path of every face's source photo, by face id: a pass that respects a catalog's switches
/// (M26.9) needs the path to ask `Settings::pass_allows`.
pub fn face_paths(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare(
        "SELECT f.id, fi.path FROM faces f JOIN files fi ON fi.id = f.file_id",
    )?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, path) = row?;
        out.insert(id, path);
    }
    Ok(out)
}

/// The path of every indexed file by id, the trashed ones included (M26.9): a pass must not fall
/// back to "allowed" for a row it did not load into its visible set.
pub fn file_paths(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT id, path FROM files")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, path) = row?;
        out.insert(id, path);
    }
    Ok(out)
}

/// What a "forget this catalog's analysis" action removed (M26.9): derived rows only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Forgotten {
    pub files: usize,
    pub tags: usize,
    pub vectors: usize,
    pub ocr: usize,
    pub faces: usize,
}

impl Forgotten {
    pub fn is_empty(&self) -> bool {
        self.tags == 0 && self.vectors == 0 && self.ocr == 0 && self.faces == 0
    }
}

/// Remove the derived analysis of one catalog: CLIP tags, vectors, recognized text, faces and the
/// scan mark. The file rows, the owner's own tags, the previews and the archive stay (M26.9).
pub fn forget_catalog_derived(conn: &Connection, root: &str) -> Result<Forgotten> {
    let ids: Vec<i64> = {
        let mut st = conn.prepare("SELECT id, path FROM files")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, path) = row?;
            if under_root(&path, root) {
                ids.push(id);
            }
        }
        ids
    };
    let mut out = Forgotten {
        files: ids.len(),
        ..Forgotten::default()
    };
    let tx = conn.unchecked_transaction()?;
    {
        let mut tags = tx.prepare("DELETE FROM tags WHERE file_id=?1")?;
        let mut vectors = tx.prepare("DELETE FROM embeddings WHERE file_id=?1")?;
        let mut ocr = tx.prepare("DELETE FROM ocr WHERE file_id=?1")?;
        let mut scans = tx.prepare("DELETE FROM face_scans WHERE file_id=?1")?;
        let mut faces = tx.prepare("DELETE FROM faces WHERE file_id=?1")?;
        let mut matches =
            tx.prepare("DELETE FROM face_matches WHERE face_id IN (SELECT id FROM faces WHERE file_id=?1)")?;
        for id in &ids {
            out.tags += tags.execute([id])?;
            out.vectors += vectors.execute([id])?;
            out.ocr += ocr.execute([id])?;
            scans.execute([id])?;
            matches.execute([id])?;
            out.faces += faces.execute([id])?;
        }
    }
    tx.commit()?;
    delete_empty_face_clusters(conn)?;
    Ok(out)
}

/// One catalogue entry.
#[derive(Debug, Clone)]
pub struct FileRow {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub sha256: String,
    pub kind: String,
    pub width: i64,
    pub height: i64,
    pub taken_at: Option<String>,
    pub taken_src: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub camera: Option<String>,
    pub dhash: Option<i64>,
    pub sharpness: Option<f64>,
/// The video duration in seconds; `NULL` for photos and for files without ffprobe.
    pub duration_s: Option<f64>,
    pub thumb: Option<String>,
/// The owner's decision on a duplicate: `keep` | `drop` | `NULL` (undecided).
    pub decision: Option<String>,
/// The time the file was put into the app trash; `NULL` means it shows in the usual views.
    #[allow(dead_code)]
    pub trashed_at: Option<i64>,
}

pub fn insert(conn: &Connection, r: &FileRow) -> Result<()> {
    conn.execute(
        "INSERT INTO files(path,size,mtime,sha256,kind,width,height,taken_at,taken_src,lat,lon,camera,dhash,sharpness,duration_s,thumb,decision,indexed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
         ON CONFLICT(path) DO UPDATE SET
            size=excluded.size, sha256=excluded.sha256, kind=excluded.kind,
            width=excluded.width, height=excluded.height, taken_at=excluded.taken_at,
            taken_src=excluded.taken_src, lat=excluded.lat, lon=excluded.lon,
            camera=excluded.camera, dhash=excluded.dhash, sharpness=excluded.sharpness,
            duration_s=excluded.duration_s,
            thumb=excluded.thumb, indexed_at=excluded.indexed_at,
            decision=CASE WHEN files.sha256 IS excluded.sha256 THEN files.decision ELSE NULL END",
        params![
            r.path,
            r.size,
            r.mtime,
            r.sha256,
            r.kind,
            r.width,
            r.height,
            r.taken_at,
            r.taken_src,
            r.lat,
            r.lon,
            r.camera,
            r.dhash,
            r.sharpness,
            r.duration_s,
            r.thumb,
            r.decision,
            now(),
        ],
    )?;
    Ok(())
}

/// Mark one file of a group "keep", the rest "spare". Originals are untouched: DB rows only.
pub fn mark_keep(db: &Path, keep: i64, drops: &[i64]) -> Result<()> {
    let mut conn = open(db)?;
    mark_keep_conn(&mut conn, keep, drops)
}

pub(super) fn mark_keep_conn(conn: &mut Connection, keep: i64, drops: &[i64]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE files SET decision='keep' WHERE id=?1",
        params![keep],
    )?;
    for &id in drops {
        tx.execute("UPDATE files SET decision='drop' WHERE id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn load_all(conn: &Connection) -> Result<Vec<FileRow>> {
    load_where(conn, "trashed_at IS NULL")
}

/// The files of the app trash: still on disk, but hidden from the usual views.
pub fn load_trashed(conn: &Connection) -> Result<Vec<FileRow>> {
    load_where(conn, "trashed_at IS NOT NULL")
}

const FILE_COLUMNS: &str = "id,path,size,sha256,kind,COALESCE(width,0),COALESCE(height,0),
                taken_at,COALESCE(taken_src,''),lat,lon,camera,dhash,sharpness,duration_s,thumb,decision,trashed_at,
                COALESCE(mtime,0)";

pub(super) fn file_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        id: r.get(0)?,
        path: r.get(1)?,
        size: r.get(2)?,
        mtime: r.get(18)?,
        sha256: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        kind: r.get(4)?,
        width: r.get(5)?,
        height: r.get(6)?,
        taken_at: r.get(7)?,
        taken_src: r.get(8)?,
        lat: r.get(9)?,
        lon: r.get(10)?,
        camera: r.get(11)?,
        dhash: r.get(12)?,
        sharpness: r.get(13)?,
        duration_s: r.get(14)?,
        thumb: r.get(15)?,
        decision: r.get(16)?,
        trashed_at: r.get(17)?,
    })
}

fn load_where(conn: &Connection, filter: &str) -> Result<Vec<FileRow>> {
    let sql = format!(
        "SELECT {FILE_COLUMNS} FROM files WHERE {filter}
         ORDER BY COALESCE(taken_at,'9999') , path"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([], file_from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Put a file into the app trash: nothing on disk changes.
pub fn mark_trashed(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "UPDATE files SET trashed_at=?1 WHERE id=?2",
        params![now(), file_id],
    )?;
    Ok(())
}

/// Bring a file back from the app trash.
pub fn untrash(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "UPDATE files SET trashed_at=NULL WHERE id=?1",
        params![file_id],
    )?;
    Ok(())
}

/// Drop a file from the index after it moved to the OS trash. The original is not ours any more.
pub fn purge_file(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    delete_file_rows(&conn, &[file_id])
}

/// Drop a folder from the index: DB rows, not files on disk.
pub fn remove_root(db: &Path, root: &str) -> Result<usize> {
    let mut conn = open(db)?;
    let root = root.trim_end_matches(['/', '\\']);
    let root = std::fs::canonicalize(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| root.to_string());
    let like = format!("{}{}%", escape_like(&root), std::path::MAIN_SEPARATOR);
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare("SELECT id FROM files WHERE path=?1 OR path LIKE ?2 ESCAPE '\\'")?;
        let rows = st.query_map(params![root, like], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()?
    };
    if ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    delete_file_rows_tx(&tx, &ids)?;
    tx.commit()?;
    Ok(ids.len())
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub(super) fn delete_file_rows(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    delete_file_rows_tx(&tx, ids)?;
    tx.commit()?;
    Ok(())
}

fn delete_file_rows_tx(tx: &Connection, ids: &[i64]) -> Result<()> {
    // Albums keep file ids only: the row is removed together with the file, otherwise
    // a dangling reference would stay and the album order would keep a hole.
    let mut albums: Vec<i64> = Vec::new();
    for id in ids {
        let file_albums: Vec<i64> = {
            let mut st =
                tx.prepare("SELECT DISTINCT album_id FROM album_files WHERE file_id=?1")?;
            let rows = st.query_map([id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for album in file_albums {
            if !albums.contains(&album) {
                albums.push(album);
            }
        }
        tx.execute("DELETE FROM album_files WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM ocr WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM embeddings WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM tags WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM tag_hidden WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM user_tags WHERE file_id=?1", params![id])?;
        tx.execute(
            "DELETE FROM face_matches WHERE face_id IN (SELECT id FROM faces WHERE file_id=?1)",
            params![id],
        )?;
        tx.execute("DELETE FROM faces WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM face_scans WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM files WHERE id=?1", params![id])?;
    }
    for album in albums {
        resequence_album_files(tx, album)?;
    }
    delete_empty_face_clusters(tx)?;
    Ok(())
}

/// Print the index statistics in the interface language (M21.6).
pub fn print_stats(lang: Lang, db_path: &Path) -> Result<()> {
    print!("{}", stats_text(lang, db_path)?);
    Ok(())
}

/// The statistics text as a separate function: the CLI prints it, a test checks the language
/// and the numbers without capturing stdout.
pub fn stats_text(lang: Lang, db_path: &Path) -> Result<String> {
    use std::fmt::Write;
    let conn = open(db_path)?;
    let mut out = String::new();
    let mut st = conn.prepare(
        "SELECT kind, COUNT(*), SUM(size) FROM files GROUP BY kind ORDER BY SUM(size) DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut total_n = 0;
    let mut total_b = 0;
    let _ = writeln!(
        out,
        "{:<10} {:>10} {:>12}",
        i18n::t(lang, "kind"),
        i18n::t(lang, "files"),
        i18n::t(lang, "size")
    );
    for r in rows {
        let (k, n, b) = r?;
        total_n += n;
        total_b += b;
        let _ = writeln!(out, "{:<10} {:>10} {:>12}", k, n, human_in(lang, b));
    }
    let no_date: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE taken_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    let no_hash: i64 =
        conn.query_row("SELECT COUNT(*) FROM files WHERE dhash IS NULL", [], |r| {
            r.get(0)
        })?;
    let uniq_sha: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT sha256) FROM files WHERE sha256 IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let uniq_dhash: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT dhash) FROM files WHERE dhash IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let _ = writeln!(out, "---");
    // A switched-off folder is named with its own count (M26.1): the rows are still in the index
    // and the totals below cover the whole DB, while the window and `index` work over the shown set.
    let settings = load_settings(&conn);
    if !settings.roots_off.is_empty() {
        let all: Vec<(String, i64)> = {
            let mut st = conn.prepare("SELECT path, size FROM files")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<(String, i64)>>>()?
        };
        let mut hidden_n = 0usize;
        let mut hidden_b = 0i64;
        for root in &settings.roots_off {
            let (n, b) = all
                .iter()
                .filter(|(p, _)| under_root(p, root))
                .fold((0usize, 0i64), |(n, b), (_, size)| (n + 1, b + size));
            hidden_n += n;
            hidden_b += b;
            let _ = writeln!(
                out,
                "{}",
                i18n::fill(
                    lang,
                    "switched off: {} — {}",
                    &[
                        root.clone(),
                        format!(
                            "{} · {}",
                            i18n::count(lang, n, "file"),
                            human_in(lang, b)
                        ),
                    ]
                )
            );
        }
        let shown_n = all.len().saturating_sub(hidden_n);
        let shown_b = all.iter().map(|(_, size)| size).sum::<i64>() - hidden_b;
        let _ = writeln!(
            out,
            "{}",
            i18n::fill(
                lang,
                "shown: {}",
                &[format!(
                    "{} · {}",
                    i18n::count(lang, shown_n, "file"),
                    human_in(lang, shown_b)
                )]
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "total: {} files, {}",
            &[total_n.to_string(), human_in(lang, total_b)]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(lang, "without a capture date: {}", &[no_date.to_string()])
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "without a perceptual hash: {}",
            &[no_hash.to_string()]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "unique sha256: {}  (exact copies: {})",
            &[uniq_sha.to_string(), (total_n - uniq_sha).to_string()]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "unique dhash: {}",
            &[uniq_dhash.to_string()]
        )
    );
    let (vid_n, vid_dur): (i64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(duration_s),0) FROM files
         WHERE kind='video' AND duration_s IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "videos with a duration: {}, {} in total",
            &[vid_n.to_string(), duration(vid_dur)]
        )
    );
    let (ocr_n, ocr_chars) = ocr_summary(&conn)?;
    if ocr_n > 0 {
        let _ = writeln!(
            out,
            "{}",
            i18n::fill(
                lang,
                "files with recognised text: {}, characters: {}",
                &[ocr_n.to_string(), ocr_chars.to_string()]
            )
        );
    }
    Ok(out)
}
