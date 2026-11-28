//! Tags, the tag dictionary, embeddings and recognized text.

use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;

use super::files::now;
use super::schema::open;

/// A tag in the dictionary: the label, the prompts and the order. It lives in the DB per
/// model (M15.4), because labels are the owner's configuration, not a constant in the code.
#[derive(Debug, Clone, PartialEq)]
pub struct TagDef {
    /// The model id (`embeddings.model`): every model has its own dictionary.
    pub model: String,
    /// The stable key of the row: the tag is renamed and removed by it.
    pub id: String,
    /// The label in Russian: it is also the key in `tags.tag`.
    pub label: String,
    /// 2–3 general templates; an empty list means the tag is off by definition.
    pub prompts: Vec<String>,
    pub enabled: bool,
    /// The order in the filter and the editor.
    pub position: i64,
    /// The minimum percentile for showing the tag (0 — whatever the slider picks).
    pub min_percentile: i32,
}

pub fn hide_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,?2,?3)
         ON CONFLICT(file_id,tag) DO NOTHING",
        params![file_id, tag, now()],
    )?;
    Ok(())
}

/// Bring back every hidden tag of a file (the original is untouched, the decision lives in the DB).
pub fn unhide_tags(db: &Path, file_id: i64) -> Result<usize> {
    let conn = open(db)?;
    Ok(conn.execute("DELETE FROM tag_hidden WHERE file_id=?1", params![file_id])?)
}

/// file_id → how many tags the owner has hidden.
pub fn load_hidden_counts(conn: &Connection) -> Result<HashMap<i64, usize>> {
    let mut st = conn.prepare("SELECT file_id, COUNT(*) FROM tag_hidden GROUP BY file_id")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, n) = r?;
        out.insert(id, n as usize);
    }
    Ok(out)
}

/// The owner's own tags: file_id → a list of labels.
pub fn load_user_tags(conn: &Connection) -> Result<HashMap<i64, Vec<String>>> {
    let mut st = conn.prepare("SELECT file_id, tag FROM user_tags ORDER BY tag")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for r in rows {
        let (id, tag) = r?;
        out.entry(id).or_default().push(tag);
    }
    Ok(out)
}

pub fn add_user_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Ok(());
    }
    let conn = open(db)?;
    conn.execute(
        "INSERT INTO user_tags(file_id,tag,created_at) VALUES(?1,?2,?3)
         ON CONFLICT(file_id,tag) DO NOTHING",
        params![file_id, tag, now()],
    )?;
    Ok(())
}

pub fn remove_user_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "DELETE FROM user_tags WHERE file_id=?1 AND tag=?2",
        params![file_id, tag],
    )?;
    Ok(())
}

/// sha256 of the vectors already computed: file_id → the file's sha256 at the time of the run.
pub fn embedding_shas(conn: &Connection, model: &str) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT file_id, sha256 FROM embeddings WHERE model=?1")?;
    let rows = st.query_map([model], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, sha) = r?;
        out.insert(id, sha);
    }
    Ok(out)
}

/// Every vector of a model: `(file_id, f32)` for cosine search and clustering.
pub fn load_embeddings(conn: &Connection, model: &str) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut st = conn.prepare("SELECT file_id, vec FROM embeddings WHERE model=?1")?;
    let rows = st.query_map([model], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id, blob) = r?;
        let vec: Vec<f32> = blob
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        out.push((id, vec));
    }
    Ok(out)
}

/// Drop the vectors of a model: a recompute with a new model input. The archive and previews are untouched.
/// Drop the vectors of one model only for the given files (M26.9): a pass switched off for a
/// catalog must not lose what a previous pass produced.
pub fn clear_embeddings_for(conn: &Connection, model: &str, ids: &[i64]) -> Result<usize> {
    let mut st = conn.prepare("DELETE FROM embeddings WHERE model=?1 AND file_id=?2")?;
    let mut n = 0;
    for id in ids {
        n += st.execute(params![model, id])?;
    }
    Ok(n)
}

pub fn save_embedding(
    conn: &Connection,
    file_id: i64,
    model: &str,
    sha256: &str,
    vec: &[f32],
) -> Result<()> {
    let mut blob = Vec::with_capacity(vec.len() * 4);
    for v in vec {
        blob.extend_from_slice(&v.to_le_bytes());
    }
    conn.execute(
        "INSERT INTO embeddings(file_id,model,sha256,dim,vec) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(file_id,model) DO UPDATE SET
            sha256=excluded.sha256, dim=excluded.dim, vec=excluded.vec",
        params![file_id, model, sha256, vec.len() as i64, blob],
    )?;
    Ok(())
}

/// Replace the tags of one source on a file (CLIP is recomputed as a whole).
pub fn replace_tags(
    conn: &Connection,
    file_id: i64,
    source: &str,
    tags: &[(String, f32)],
) -> Result<()> {
    conn.execute(
        "DELETE FROM tags WHERE file_id=?1 AND source=?2",
        params![file_id, source],
    )?;
    let mut st = conn.prepare("INSERT INTO tags(file_id,tag,score,source) VALUES(?1,?2,?3,?4)")?;
    for (tag, score) in tags {
        st.execute(params![file_id, tag, score, source])?;
    }
    Ok(())
}

/// Every tag: file_id → [(tag, weight)] by descending weight.
/// Every visible tag of the chosen source: file_id → [(tag, weight)] by descending weight.
/// Tags hidden by the owner (`tag_hidden`) do not appear in the result.
pub fn load_tags(conn: &Connection, source: &str) -> Result<HashMap<i64, Vec<(String, f32)>>> {
    let mut st = conn.prepare(
        "SELECT t.file_id, t.tag, COALESCE(t.score,0)
         FROM tags t
         LEFT JOIN tag_hidden h ON h.file_id=t.file_id AND h.tag=t.tag
         WHERE t.source=?1 AND h.tag IS NULL
         ORDER BY t.file_id, t.score DESC",
    )?;
    let rows = st.query_map([source], |r| {
        Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?))
    })?;
    let mut out: HashMap<i64, Vec<(String, f32)>> = HashMap::new();
    for r in rows {
        let (id, tag, score) = r?;
        out.entry(id).or_default().push((tag, score));
    }
    Ok(out)
}

/// A file fit for the labelled sample M15.7: an image not in the trash, with a preview and with
/// the model's tags. The order is by sha256: the sample does not depend on names or dates and repeats exactly.
#[derive(Debug, Clone)]
pub struct TaggedFile {
    pub id: i64,
    pub path: String,
    pub sha256: String,
    /// The preview path relative to `<cache>/photocat/thumbs`.
    pub thumb: Option<String>,
}

pub fn tagged_files(conn: &Connection, source: &str) -> Result<Vec<TaggedFile>> {
    let mut st = conn.prepare(
        "SELECT f.id, f.path, f.sha256, f.thumb
         FROM files f
         WHERE f.kind='image' AND f.thumb IS NOT NULL AND f.trashed_at IS NULL
           AND f.sha256 IS NOT NULL AND f.sha256 <> ''
           AND EXISTS(SELECT 1 FROM tags t WHERE t.file_id=f.id AND t.source=?1)
         ORDER BY f.sha256",
    )?;
    let rows = st.query_map([source], |r| {
        Ok(TaggedFile {
            id: r.get(0)?,
            path: r.get(1)?,
            sha256: r.get(2)?,
            thumb: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// sha256 of the text already recognized for a file: file_id → the file's sha256 at the time of the OCR.
pub fn ocr_shas(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT file_id, sha256 FROM ocr")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, sha) = r?;
        out.insert(id, sha);
    }
    Ok(out)
}

/// Store the recognized text. The text lives in the DB: nothing is written next to the original.
pub fn save_ocr(
    conn: &Connection,
    file_id: i64,
    sha256: &str,
    lang: &str,
    text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ocr(file_id,sha256,lang,text,chars,created_at)
         VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(file_id) DO UPDATE SET
            sha256=excluded.sha256, lang=excluded.lang, text=excluded.text,
            chars=excluded.chars, created_at=excluded.created_at",
        params![
            file_id,
            sha256,
            lang,
            text,
            text.chars().count() as i64,
            now()
        ],
    )?;
    Ok(())
}

pub fn load_ocr(conn: &Connection, file_id: i64) -> Result<Option<String>> {
    let mut st = conn.prepare("SELECT text FROM ocr WHERE file_id=?1")?;
    let mut rows = st.query([file_id])?;
    match rows.next()? {
        Some(r) => Ok(Some(r.get(0)?)),
        None => Ok(None),
    }
}

/// How many files have text and how many characters were recognized in all.
pub fn ocr_summary(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(chars),0) FROM ocr",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// The recognized text of every file, file_id → characters: the window counts the files with
/// text over the visible set (M26.1), not over the whole DB.
pub fn ocr_file_counts(conn: &Connection) -> Result<HashMap<i64, i64>> {
    let mut st = conn.prepare("SELECT file_id, chars FROM ocr")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, chars) = r?;
        out.insert(id, chars);
    }
    Ok(out)
}

/// The tag dictionary of a model in display order. Empty means the model has not been seeded yet.
pub fn load_tag_defs(conn: &Connection, model: &str) -> Result<Vec<TagDef>> {
    let mut st = conn.prepare(
        "SELECT tag_id, label, prompts, enabled, position, min_pct
         FROM tag_defs WHERE model=?1 ORDER BY position, label",
    )?;
    let rows = st.query_map([model], |r| {
        Ok(TagDef {
            model: model.to_string(),
            id: r.get(0)?,
            label: r.get(1)?,
            prompts: r
                .get::<_, String>(2)?
                .lines()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
            enabled: r.get::<_, i64>(3)? != 0,
            position: r.get(4)?,
            min_percentile: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// The labels every dictionary gives the tags `document` and `screenshot`: the ids are stable, the
/// labels are the owner's (and follow the language of the defaults). Every model's dictionary is
/// read on purpose — `settings.model_id` is empty until the owner picks a model, so resolving the
/// dictionary through the setting silently found nothing and OCR lost its content-based candidates
/// (M23.6). The labels of these two ids cannot widen the heuristic beyond documents.
pub fn text_tag_labels(conn: &Connection) -> Vec<String> {
    let Ok(mut st) = conn.prepare(
        "SELECT DISTINCT label FROM tag_defs
         WHERE enabled<>0 AND tag_id IN ('document','screenshot') ORDER BY label",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = st.query_map([], |r| r.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// Store a dictionary tag in place (by `model`+`id`) without touching its neighbours.
/// The label is unique inside a model: otherwise `tags` would get two rows with one key.
pub fn save_tag_def(conn: &Connection, d: &TagDef) -> Result<()> {
    let taken: Option<String> = conn
        .query_row(
            "SELECT tag_id FROM tag_defs WHERE model=?1 AND label=?2 AND tag_id<>?3",
            params![d.model, d.label, d.id],
            |r| r.get(0),
        )
        .ok();
    if let Some(other) = taken {
        anyhow::bail!("the label “{}” is already taken by tag {other}", d.label);
    }
    let prompts = d
        .prompts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    conn.execute(
        "INSERT INTO tag_defs(model,tag_id,label,prompts,enabled,position,min_pct)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(model,tag_id) DO UPDATE SET
            label=excluded.label, prompts=excluded.prompts, enabled=excluded.enabled,
            position=excluded.position, min_pct=excluded.min_pct",
        params![
            d.model,
            d.id,
            d.label,
            prompts,
            d.enabled as i64,
            d.position,
            d.min_percentile.clamp(0, 100)
        ],
    )?;
    Ok(())
}

/// Remove a tag from the dictionary. The `tags` rows with this label are deleted by the tag recompute.
pub fn delete_tag_def(conn: &Connection, model: &str, id: &str) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM tag_defs WHERE model=?1 AND tag_id=?2",
        params![model, id],
    )?)
}

/// Replace the whole dictionary of a model (JSON import, "back to the default tags").
pub fn replace_tag_defs(conn: &Connection, model: &str, defs: &[TagDef]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM tag_defs WHERE model=?1", [model])?;
    for d in defs {
        save_tag_def(&tx, d)?;
    }
    tx.commit()?;
    Ok(())
}

/// How many files each tag of the source has — for `tags --list` and an empty dictionary.
pub fn tag_file_counts(conn: &Connection, source: &str) -> Result<HashMap<String, usize>> {
    let mut st = conn.prepare("SELECT tag, COUNT(*) FROM tags WHERE source=?1 GROUP BY tag")?;
    let rows = st.query_map([source], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (tag, n) = r?;
        out.insert(tag, n as usize);
    }
    Ok(out)
}

/// Every visible tag with weights: file_id → [(tag, weight)] by descending weight.
/// One tag from several models is taken by the best weight, so guesses are not duplicated.
/// The OCR pass needs it: the tag "document"/"screenshot" is the reason to look for text.
pub fn load_visible_tags(conn: &Connection) -> Result<HashMap<i64, Vec<(String, f32)>>> {
    let mut st = conn.prepare(
        "WITH visible AS (
            SELECT t.file_id, t.tag, MAX(COALESCE(t.score,0)) AS score
            FROM tags t
            LEFT JOIN tag_hidden h ON h.file_id=t.file_id AND h.tag=t.tag
            WHERE h.tag IS NULL
            GROUP BY t.file_id, t.tag
         )
         SELECT file_id, tag, score FROM visible ORDER BY file_id, score DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f32>(2)?,
        ))
    })?;
    let mut out: HashMap<i64, Vec<(String, f32)>> = HashMap::new();
    for r in rows {
        let (id, tag, score) = r?;
        out.entry(id).or_default().push((tag, score));
    }
    Ok(out)
}

/// An FTS5 query: every word is a prefix, quotes strip the special characters.
/// An empty string means `None`, not an FTS syntax error.
pub fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| t.replace(['"', '*'], ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

/// Full-text search over recognized text: `(file_id, a fragment with "…")`.
/// The fragment is already marked up: the UI colours the matches, the CLI prints it as is.
pub fn search_ocr(conn: &Connection, q: &str, limit: usize) -> Result<Vec<(i64, String)>> {
    let Some(match_q) = fts_query(q) else {
        return Ok(Vec::new());
    };
    let mut st = conn.prepare(
        "SELECT o.file_id, snippet(ocr_fts, 0, '«', '»', '…', 12)
         FROM ocr_fts JOIN ocr o ON o.file_id = ocr_fts.rowid
         WHERE ocr_fts MATCH ?1
         ORDER BY rank LIMIT ?2",
    )?;
    let rows = st.query_map(params![match_q, limit as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}
