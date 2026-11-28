//! The album tree and the file order inside an album: rows only.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

use super::files::{FileRow, file_from_row, now};
use super::settings::AlbumSort;

/// An album tree node (M19). An album is DB rows only: no file on disk is created,
/// changed or moved.
#[derive(Debug, Clone)]
pub struct AlbumNode {
    pub id: i64,
    /// 0 is the root of the tree.
    pub parent_id: i64,
    pub position: i64,
    pub name: String,
    pub note: String,
    /// The depth in the tree: 0 at the root. The interface needs it for the indent.
    pub depth: usize,
    /// How many files the album has (including those in the app trash).
    pub files: i64,
    /// How many of them are in the app trash: the grid hides those.
    pub trashed: i64,
    pub subalbums: i64,
    pub bytes: i64,
    /// Unix seconds of the last change of the album or of its contents: the card shows the date
    /// and "by update" sorts by it (M22.20).
    pub updated_at: String,
    /// Up to four previews of the first files in order that are not in the trash, in album order:
    /// the album card is a mosaic of them (M22.21). Empty for an album without previews.
    pub covers: Vec<String>,
}

/// How many previews of an album a card shows: one photo fills it, four split it into a 2×2
/// mosaic (M22.21). A larger album keeps the first photos in its order.
pub const ALBUM_COVERS: usize = 4;

/// The order of two sibling albums for the cards (M22.20). A tie keeps the album order, so the
/// screen does not shuffle on equal numbers.
pub fn album_order(a: &AlbumNode, b: &AlbumNode, sort: AlbumSort) -> std::cmp::Ordering {
    let by_position = || (a.position, a.id).cmp(&(b.position, b.id));
    let stamp = |n: &AlbumNode| n.updated_at.parse::<i64>().unwrap_or(0);
    match sort {
        AlbumSort::Position => by_position(),
        AlbumSort::Updated => stamp(b).cmp(&stamp(a)).then_with(by_position),
        AlbumSort::Name => a
            .name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(by_position),
        AlbumSort::Count => b.files.cmp(&a.files).then_with(by_position),
        AlbumSort::Size => b.bytes.cmp(&a.bytes).then_with(by_position),
    }
}

fn album_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("the album name is empty");
    }
    if name.chars().count() > 80 {
        anyhow::bail!("the album name is longer than 80 characters");
    }
    if name.chars().any(char::is_control) {
        anyhow::bail!("the album name contains a control character");
    }
    Ok(name.to_string())
}

fn album_exists(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM albums WHERE id=?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn sibling_name_free(conn: &Connection, parent: i64, name_key: &str, except: i64) -> Result<()> {
    let taken: Option<i64> = conn
        .query_row(
            "SELECT id FROM albums WHERE parent_id=?1 AND name_key=?2 AND id<>?3",
            params![parent, name_key, except],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = taken {
        anyhow::bail!("an album with this name already exists among the siblings (id {id})");
    }
    Ok(())
}

fn resequence_albums(conn: &Connection, parent: i64) -> Result<()> {
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare("SELECT id FROM albums WHERE parent_id=?1 ORDER BY position, id")?;
        let rows = st.query_map([parent], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE albums SET position=?1 WHERE id=?2",
            params![position as i64, id],
        )?;
    }
    Ok(())
}

pub(super) fn resequence_album_files(conn: &Connection, album_id: i64) -> Result<()> {
    let ids: Vec<i64> = {
        let mut st = conn.prepare(
            "SELECT file_id FROM album_files WHERE album_id=?1 ORDER BY position, file_id",
        )?;
        let rows = st.query_map([album_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, file_id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE album_files SET position=?1 WHERE album_id=?2 AND file_id=?3",
            params![position as i64, album_id, file_id],
        )?;
    }
    Ok(())
}

fn album_rows(conn: &Connection) -> Result<HashMap<i64, (i64, String)>> {
    let mut st = conn.prepare("SELECT id, parent_id, name FROM albums")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, parent, name) = row?;
        map.insert(id, (parent, name));
    }
    Ok(map)
}

fn path_in(map: &HashMap<i64, (i64, String)>, id: i64) -> String {
    let mut names = Vec::new();
    let mut at = id;
    for _ in 0..256 {
        let Some((parent, name)) = map.get(&at) else {
            break;
        };
        names.push(name.clone());
        if *parent == 0 {
            break;
        }
        at = *parent;
    }
    names.reverse();
    names.join(" / ")
}

/// The path of an album in the tree: "Dacha 2024 / May".
pub fn album_path(conn: &Connection, id: i64) -> Result<String> {
    let map = album_rows(conn)?;
    if !map.contains_key(&id) {
        anyhow::bail!("there is no album {id}");
    }
    Ok(path_in(&map, id))
}

/// Create an album at the end of its siblings. `parent_id = 0` is the root of the tree.
pub fn create_album(conn: &Connection, parent_id: i64, name: &str, note: &str) -> Result<i64> {
    let name = album_name(name)?;
    let tx = conn.unchecked_transaction()?;
    if parent_id != 0 && !album_exists(&tx, parent_id)? {
        anyhow::bail!("there is no parent album {parent_id}");
    }
    let key = name.to_lowercase();
    sibling_name_free(&tx, parent_id, &key, 0)?;
    let position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position),-1)+1 FROM albums WHERE parent_id=?1",
        [parent_id],
        |r| r.get(0),
    )?;
    let stamp = now().to_string();
    tx.execute(
        "INSERT INTO albums(parent_id,position,name,name_key,note,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?6)",
        params![parent_id, position, name, key, note.trim(), stamp],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

/// Rename an album. The name is unique among the siblings, case-insensitively.
pub fn rename_album(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = album_name(name)?;
    let tx = conn.unchecked_transaction()?;
    let parent: i64 = tx
        .query_row("SELECT parent_id FROM albums WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .context("there is no album")?;
    let key = name.to_lowercase();
    sibling_name_free(&tx, parent, &key, id)?;
    tx.execute(
        "UPDATE albums SET name=?1, name_key=?2, updated_at=?3 WHERE id=?4",
        params![name, key, now().to_string(), id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Move an album to another parent. `position = None` means the end.
/// The new parent cannot be the album itself or its descendant.
pub fn move_album(
    conn: &Connection,
    id: i64,
    new_parent: i64,
    position: Option<i64>,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let (old_parent, name_key): (i64, String) = tx
        .query_row(
            "SELECT parent_id, name_key FROM albums WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .context("there is no album")?;
    if new_parent == id {
        anyhow::bail!("an album cannot be moved into itself");
    }
    if new_parent != 0 && !album_exists(&tx, new_parent)? {
        anyhow::bail!("there is no parent album {new_parent}");
    }
    // A cycle: walk up from the new parent; reaching the id means a refusal.
    let mut at = new_parent;
    for _ in 0..1000 {
        if at == 0 {
            break;
        }
        let parent: Option<i64> = tx
            .query_row("SELECT parent_id FROM albums WHERE id=?1", [at], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(parent) = parent else {
            break;
        };
        if parent == id {
            anyhow::bail!("an album cannot be moved into its own subalbum");
        }
        at = parent;
    }
    sibling_name_free(&tx, new_parent, &name_key, id)?;
    let mut siblings: Vec<i64> = {
        let mut st = tx.prepare(
            "SELECT id FROM albums WHERE parent_id=?1 AND id<>?2 ORDER BY position, id",
        )?;
        let rows = st.query_map(params![new_parent, id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let at = position
        .unwrap_or(siblings.len() as i64)
        .clamp(0, siblings.len() as i64) as usize;
    siblings.insert(at, id);
    tx.execute(
        "UPDATE albums SET parent_id=?1, updated_at=?2 WHERE id=?3",
        params![new_parent, now().to_string(), id],
    )?;
    for (p, sibling) in siblings.iter().enumerate() {
        tx.execute(
            "UPDATE albums SET position=?1 WHERE id=?2",
            params![p as i64, sibling],
        )?;
    }
    if old_parent != new_parent {
        resequence_albums(&tx, old_parent)?;
    }
    tx.commit()?;
    Ok(())
}

/// Delete an album with its whole subtree. Rows only: the files on disk are intact.
pub fn delete_album(conn: &Connection, id: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let parent: i64 = tx
        .query_row("SELECT parent_id FROM albums WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .context("there is no album")?;
    let ids: Vec<i64> = {
        let mut st = tx.prepare(
            "WITH RECURSIVE sub(id) AS (
                SELECT id FROM albums WHERE id=?1
                UNION ALL
                SELECT a.id FROM albums a JOIN sub ON a.parent_id=sub.id
             ) SELECT id FROM sub",
        )?;
        let rows = st.query_map([id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for album in &ids {
        tx.execute("DELETE FROM album_files WHERE album_id=?1", [album])?;
        tx.execute("DELETE FROM albums WHERE id=?1", [album])?;
    }
    resequence_albums(&tx, parent)?;
    tx.commit()?;
    Ok(ids.len())
}

/// Append files to an album. A repeat does not duplicate rows: it returns
/// `(added, already there)`. Files missing from the index cannot appear here.
pub fn add_to_album(conn: &Connection, album_id: i64, file_ids: &[i64]) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;
    if !album_exists(&tx, album_id)? {
        anyhow::bail!("there is no album {album_id}");
    }
    let mut position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position),-1)+1 FROM album_files WHERE album_id=?1",
        [album_id],
        |r| r.get(0),
    )?;
    let mut added = 0usize;
    let mut already = 0usize;
    let mut seen = std::collections::HashSet::new();
    for &file_id in file_ids {
        if !seen.insert(file_id) {
            continue;
        }
        let in_index: bool = tx
            .query_row("SELECT 1 FROM files WHERE id=?1", [file_id], |_| Ok(()))
            .optional()?
            .is_some();
        if !in_index {
            continue;
        }
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM album_files WHERE album_id=?1 AND file_id=?2",
                params![album_id, file_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            already += 1;
            continue;
        }
        tx.execute(
            "INSERT INTO album_files(album_id,file_id,position,added_at) VALUES(?1,?2,?3,?4)",
            params![album_id, file_id, position, now().to_string()],
        )?;
        position += 1;
        added += 1;
    }
    // The card of the album shows the date of its last change (M22.20).
    if added > 0 {
        tx.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params![now().to_string(), album_id],
        )?;
    }
    tx.commit()?;
    Ok((added, already))
}

/// Remove files from an album and compact the order. Returns the number of deleted rows.
pub fn remove_from_album(conn: &Connection, album_id: i64, file_ids: &[i64]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut removed = 0usize;
    for &file_id in file_ids {
        removed += tx.execute(
            "DELETE FROM album_files WHERE album_id=?1 AND file_id=?2",
            params![album_id, file_id],
        )?;
    }
    if removed > 0 {
        resequence_album_files(&tx, album_id)?;
        tx.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params![now().to_string(), album_id],
        )?;
    }
    tx.commit()?;
    Ok(removed)
}

/// "Up/Down" inside an album: `delta` is −1 or +1. The order becomes dense 0..n−1 again.
pub fn move_in_album(
    conn: &Connection,
    album_id: i64,
    file_id: i64,
    delta: i64,
) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let mut ids: Vec<i64> = {
        let mut st = tx.prepare(
            "SELECT file_id FROM album_files WHERE album_id=?1 ORDER BY position, file_id",
        )?;
        let rows = st.query_map([album_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let Some(at) = ids.iter().position(|&id| id == file_id) else {
        return Ok(false);
    };
    let to = (at as i64 + delta).clamp(0, ids.len() as i64 - 1) as usize;
    if at == to {
        return Ok(false);
    }
    let moved = ids.remove(at);
    ids.insert(to, moved);
    for (position, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE album_files SET position=?1 WHERE album_id=?2 AND file_id=?3",
            params![position as i64, album_id, id],
        )?;
    }
    tx.execute(
        "UPDATE albums SET updated_at=?1 WHERE id=?2",
        params![now().to_string(), album_id],
    )?;
    tx.commit()?;
    Ok(true)
}

/// The album tree in traversal order: the roots by `position`, then their subalbums.
/// `depth` is the indent for the interface; the counters are the rows and the size of the album.
pub fn albums_tree(conn: &Connection) -> Result<Vec<AlbumNode>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.parent_id, a.position, a.name, a.note, a.updated_at,
                (SELECT COUNT(*) FROM album_files af WHERE af.album_id=a.id),
                (SELECT COUNT(*) FROM album_files af JOIN files f ON f.id=af.file_id
                  WHERE af.album_id=a.id AND f.trashed_at IS NOT NULL),
                (SELECT COUNT(*) FROM albums c WHERE c.parent_id=a.id),
                (SELECT COALESCE(SUM(f.size),0) FROM album_files af
                  JOIN files f ON f.id=af.file_id WHERE af.album_id=a.id)
         FROM albums a ORDER BY a.parent_id, a.position, a.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(AlbumNode {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            position: r.get(2)?,
            name: r.get(3)?,
            note: r.get(4)?,
            updated_at: r.get(5)?,
            depth: 0,
            files: r.get(6)?,
            trashed: r.get(7)?,
            subalbums: r.get(8)?,
            bytes: r.get(9)?,
            covers: Vec::new(),
        })
    })?;
    // The previews of every album in one pass, at most `ALBUM_COVERS` per album and in the album
    // order. A file without a preview is skipped, not replaced by a placeholder.
    let mut covers: HashMap<i64, Vec<String>> = HashMap::new();
    let mut cover_st = conn.prepare(
        "SELECT af.album_id, f.thumb FROM album_files af JOIN files f ON f.id=af.file_id
         WHERE f.trashed_at IS NULL AND f.thumb IS NOT NULL
         ORDER BY af.album_id, af.position, f.id",
    )?;
    let cover_rows = cover_st.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in cover_rows {
        let (album, thumb) = row?;
        let list = covers.entry(album).or_default();
        if list.len() < ALBUM_COVERS {
            list.push(thumb);
        }
    }
    let mut by_parent: HashMap<i64, Vec<AlbumNode>> = HashMap::new();
    for row in rows {
        let mut node = row?;
        node.covers = covers.remove(&node.id).unwrap_or_default();
        by_parent.entry(node.parent_id).or_default().push(node);
    }
    fn walk(
        parent: i64,
        depth: usize,
        by_parent: &mut HashMap<i64, Vec<AlbumNode>>,
        out: &mut Vec<AlbumNode>,
    ) {
        let Some(nodes) = by_parent.remove(&parent) else {
            return;
        };
        for mut node in nodes {
            node.depth = depth;
            let id = node.id;
            out.push(node);
            walk(id, depth + 1, by_parent, out);
        }
    }
    let mut out = Vec::new();
    walk(0, 0, &mut by_parent, &mut out);
    // A row with a lost parent must not disappear from the interface.
    while let Some(&parent) = by_parent.keys().min() {
        walk(parent, 0, &mut by_parent, &mut out);
    }
    Ok(out)
}

/// The files of an album in its order. Includes the app trash rows: the interface
/// hides them in the grid and counts them separately, while the data stays.
pub fn album_items(conn: &Connection, album_id: i64) -> Result<Vec<FileRow>> {
    let sql = "SELECT f.id, f.path, f.size, f.sha256, f.kind, COALESCE(f.width,0),
                COALESCE(f.height,0), f.taken_at, COALESCE(f.taken_src,''), f.lat, f.lon,
                f.camera, f.dhash, f.sharpness, f.duration_s, f.thumb, f.decision,
                f.trashed_at, COALESCE(f.mtime,0)
         FROM album_files af JOIN files f ON f.id=af.file_id
         WHERE af.album_id=?1 ORDER BY af.position, f.id";
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map([album_id], file_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The subtree of an album in tree order: the root, then its subalbums.
/// The export needs it (M20): the structure on disk repeats this tree.
pub fn album_subtree(conn: &Connection, id: i64) -> Result<Vec<AlbumNode>> {
    let tree = albums_tree(conn)?;
    if !tree.iter().any(|n| n.id == id) {
        anyhow::bail!("there is no album {id}");
    }
    let mut ids = vec![id];
    loop {
        let mut grown = false;
        for node in &tree {
            if ids.contains(&node.parent_id) && !ids.contains(&node.id) {
                ids.push(node.id);
                grown = true;
            }
        }
        if !grown {
            break;
        }
    }
    Ok(tree.into_iter().filter(|n| ids.contains(&n.id)).collect())
}

/// Data for the album export (M20): the nodes of the subtree and their files in album
/// order. Read-only; not a single file on disk is opened.
pub fn album_export_data(
    conn: &Connection,
    id: i64,
) -> Result<Vec<(AlbumNode, Vec<FileRow>)>> {
    let mut out = Vec::new();
    for node in album_subtree(conn, id)? {
        let files = album_items(conn, node.id)?;
        out.push((node, files));
    }
    Ok(out)
}

/// The albums a file lies in: the id and the path in the tree. For the inspector and the card.
pub fn album_refs_of_file(conn: &Connection, file_id: i64) -> Result<Vec<(i64, String)>> {
    let map = album_rows(conn)?;
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare("SELECT album_id FROM album_files WHERE file_id=?1 ORDER BY album_id")?;
        let rows = st.query_map([file_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut out: Vec<(i64, String)> = ids
        .into_iter()
        .filter(|id| map.contains_key(id))
        .map(|id| (id, path_in(&map, id)))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

/// Only the album paths of a file — for the inspector (M19). The card takes
/// `album_refs_of_file`, because the "×" needs the id; this form is for reports.
#[allow(dead_code)]
pub fn albums_of_file(conn: &Connection, file_id: i64) -> Result<Vec<String>> {
    Ok(album_refs_of_file(conn, file_id)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}
