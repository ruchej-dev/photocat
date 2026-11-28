//! Detected faces, their groups and the quarantine of automatic matches.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

use super::files::now;

/// One detected face: the box, the embedding and the cluster. The vector is f32 in a BLOB.
#[derive(Debug, Clone)]
pub struct FaceRow {
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub score: f32,
    pub quality: f32,
    pub thumb: Option<String>,
    pub embedding: Vec<f32>,
    pub cluster_id: Option<i64>,
    pub manual: bool,
}

/// A face for the interface: no embedding, but with the path and the date of the source file.
#[derive(Debug, Clone)]
pub struct FaceView {
    pub id: i64,
    pub file_id: i64,
    pub path: String,
    pub taken_at: Option<String>,
    pub thumb: Option<String>,
    pub score: f32,
    pub cluster_id: Option<i64>,
}

/// A face that the matching step proposes for a named person (M26.17): it stays where it is — in
/// its own group or without one — and waits in the quarantine until the owner confirms it or turns
/// it down. Nothing has moved yet.
#[derive(Debug, Clone)]
pub struct FaceProposal {
    pub face: FaceView,
    /// The named group the face resembles.
    pub target_id: i64,
    pub target_label: String,
    /// The cosine of the closest member of that group.
    pub similarity: f32,
}

/// A face box over the frame of its file (M22.7): the photo in the full screen marks who is
/// in it. The coordinates are image pixels of the file as it is seen (EXIF orientation applied).
#[derive(Debug, Clone)]
pub struct FaceBox {
    /// The row of `faces`: a click on the box opens the photos of this very face (M22.28).
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub score: f32,
    /// The group of the face; `None` while it is not clustered yet.
    pub cluster_id: Option<i64>,
    /// The name of the group; empty while the group has no name.
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct FaceCluster {
    pub id: i64,
    pub label: String,
    pub faces: i64,
    pub photos: i64,
}

fn f32_blob(v: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(v.len() * 4);
    for x in v {
        blob.extend_from_slice(&x.to_le_bytes());
    }
    blob
}

fn f32_from_blob(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

/// sha256 of the files already scanned for faces: file_id → sha256 at the time of the pass.
pub fn face_scan_shas(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT file_id, sha256 FROM face_scans")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, sha) = r?;
        out.insert(id, sha);
    }
    Ok(out)
}

/// Replace the faces of one file and mark it scanned. An empty list is a result too.
pub fn save_faces_for_file(
    conn: &Connection,
    file_id: i64,
    sha256: &str,
    faces: &[FaceRow],
) -> Result<()> {
    conn.execute(
        "DELETE FROM face_matches WHERE face_id IN (SELECT id FROM faces WHERE file_id=?1)",
        params![file_id],
    )?;
    conn.execute("DELETE FROM faces WHERE file_id=?1", params![file_id])?;
    {
        let mut st = conn.prepare(
            "INSERT INTO faces(file_id,sha256,x,y,w,h,score,quality,thumb,embedding,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )?;
        for f in faces {
            st.execute(params![
                file_id,
                sha256,
                f.x,
                f.y,
                f.w,
                f.h,
                f.score,
                f.quality,
                f.thumb,
                f32_blob(&f.embedding),
                now(),
            ])?;
        }
    }
    conn.execute(
        "INSERT INTO face_scans(file_id,sha256,faces,created_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(file_id) DO UPDATE SET
            sha256=excluded.sha256, faces=excluded.faces, created_at=excluded.created_at",
        params![file_id, sha256, faces.len() as i64, now()],
    )?;
    Ok(())
}

/// Every face with its embedding — for clustering.
pub fn load_faces(conn: &Connection) -> Result<Vec<FaceRow>> {
    let mut st = conn.prepare(
        "SELECT id,x,y,w,h,score,quality,thumb,embedding,cluster_id,manual
         FROM faces ORDER BY id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FaceRow {
            id: r.get(0)?,
            x: r.get(1)?,
            y: r.get(2)?,
            w: r.get(3)?,
            h: r.get(4)?,
            score: r.get(5)?,
            quality: r.get(6)?,
            thumb: r.get(7)?,
            embedding: f32_from_blob(&r.get::<_, Vec<u8>>(8)?),
            cluster_id: r.get(9)?,
            manual: r.get::<_, i64>(10)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Faces for the grid: `cluster_id=None` means all. The large and confident ones come first.
pub fn load_face_views(conn: &Connection, cluster_id: Option<i64>) -> Result<Vec<FaceView>> {
    let mut st = conn.prepare(
        "SELECT f.id,f.file_id,fi.path,fi.taken_at,f.thumb,f.score,f.cluster_id
         FROM faces f JOIN files fi ON fi.id=f.file_id
         WHERE ?1 IS NULL OR f.cluster_id=?1
         ORDER BY f.quality DESC, f.id",
    )?;
    let rows = st.query_map([cluster_id], |r| {
        Ok(FaceView {
            id: r.get(0)?,
            file_id: r.get(1)?,
            path: r.get(2)?,
            taken_at: r.get(3)?,
            thumb: r.get(4)?,
            score: r.get(5)?,
            cluster_id: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// The faces waiting in the quarantine, grouped by the person they resemble (M26.17): the closest
/// proposal of every person first, then the rest. The face is still in its own group — a proposal
/// is not a membership.
pub fn load_face_proposals(conn: &Connection) -> Result<Vec<FaceProposal>> {
    let mut st = conn.prepare(
        "SELECT f.id,f.file_id,fi.path,fi.taken_at,f.thumb,f.score,f.cluster_id,
                m.cluster_id,COALESCE(c.label,''),m.score
         FROM face_matches m
         JOIN faces f ON f.id=m.face_id
         JOIN files fi ON fi.id=f.file_id
         JOIN face_clusters c ON c.id=m.cluster_id
         ORDER BY c.label, m.score DESC, f.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FaceProposal {
            face: FaceView {
                id: r.get(0)?,
                file_id: r.get(1)?,
                path: r.get(2)?,
                taken_at: r.get(3)?,
                thumb: r.get(4)?,
                score: r.get(5)?,
                cluster_id: r.get(6)?,
            },
            target_id: r.get(7)?,
            target_label: r.get(8)?,
            similarity: r.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Write the proposals of one matching step and drop every older one (M26.17): the step is
/// deterministic, so a proposal the owner has not answered yet is the same proposal again. A
/// person who lost the name leaves the table — no proposal without a name.
pub fn replace_face_matches(
    conn: &mut Connection,
    matches: &[(i64, i64, f32)],
) -> Result<usize> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM face_matches", [])?;
    {
        let mut st = tx.prepare(
            "INSERT INTO face_matches(face_id,cluster_id,score,created_at) VALUES(?1,?2,?3,?4)",
        )?;
        for &(face_id, cluster_id, score) in matches {
            st.execute(params![face_id, cluster_id, score, now()])?;
        }
    }
    tx.commit()?;
    Ok(matches.len())
}

/// The owner confirmed the proposals of these faces (M26.17): each moves into the named group it
/// resembles and is pinned (`manual=1`), so a rebuild keeps the decision. The proposal rows go
/// away, and a group the move emptied is swept. Rows only — no face crop and no photo is touched.
pub fn confirm_face_matches(conn: &mut Connection, face_ids: &[i64]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut moved = 0usize;
    {
        // A group that lost its name in the meantime is not a person: the proposal is dropped
        // rather than confirmed into an unnamed group (M26.17).
        let mut target = tx.prepare(
            "SELECT m.cluster_id FROM face_matches m JOIN face_clusters c ON c.id=m.cluster_id
             WHERE m.face_id=?1 AND c.label<>''",
        )?;
        let mut apply = tx.prepare(
            "UPDATE faces SET cluster_id=?1, manual=1, match_score=NULL WHERE id=?2",
        )?;
        let mut drop_it = tx.prepare("DELETE FROM face_matches WHERE face_id=?1")?;
        for &face_id in face_ids {
            let cluster: Option<i64> = target
                .query_row([face_id], |r| r.get(0))
                .optional()?;
            if let Some(cluster_id) = cluster
                && apply.execute(params![cluster_id, face_id])? > 0
            {
                moved += 1;
            }
            drop_it.execute([face_id])?;
        }
    }
    tx.commit()?;
    delete_empty_face_clusters(conn)?;
    Ok(moved)
}

/// The owner turned the proposals of these faces down (M26.17): they stay in their own groups and
/// are pinned by hand, so the next matching step does not bring them back. Rows only.
pub fn reject_face_matches(conn: &mut Connection, face_ids: &[i64]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut pinned = 0usize;
    {
        let mut pin = tx.prepare("UPDATE faces SET manual=1, match_score=NULL WHERE id=?1")?;
        let mut drop_it = tx.prepare("DELETE FROM face_matches WHERE face_id=?1")?;
        for &face_id in face_ids {
            pinned += pin.execute([face_id])?;
            drop_it.execute([face_id])?;
        }
    }
    tx.commit()?;
    Ok(pinned)
}

/// Forget every proposal: the faces stay where they are. Used when there is nothing to match to.
pub fn clear_face_matches(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM face_matches", [])?)
}

/// The faces of one file with the names of their groups: the chips over the frame in the full
/// screen. An unnamed group gives an empty label.
pub fn load_face_boxes(conn: &Connection, file_id: i64) -> Result<Vec<FaceBox>> {
    let mut st = conn.prepare(
        "SELECT f.id,f.x,f.y,f.w,f.h,f.score,f.cluster_id,COALESCE(c.label,'')
         FROM faces f LEFT JOIN face_clusters c ON c.id=f.cluster_id
         WHERE f.file_id=?1 ORDER BY f.quality DESC, f.id",
    )?;
    let rows = st.query_map([file_id], |r| {
        Ok(FaceBox {
            id: r.get(0)?,
            x: r.get(1)?,
            y: r.get(2)?,
            w: r.get(3)?,
            h: r.get(4)?,
            score: r.get(5)?,
            cluster_id: r.get(6)?,
            label: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn load_face_clusters(conn: &Connection) -> Result<Vec<FaceCluster>> {
    let mut st = conn.prepare(
        "SELECT c.id,c.label,COUNT(f.id),COUNT(DISTINCT f.file_id)
         FROM face_clusters c JOIN faces f ON f.cluster_id=c.id
         GROUP BY c.id ORDER BY c.label='', c.label, COUNT(DISTINCT f.file_id) DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FaceCluster {
            id: r.get(0)?,
            label: r.get(1)?,
            faces: r.get(2)?,
            photos: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn create_face_cluster(conn: &Connection) -> Result<i64> {
    conn.execute(
        "INSERT INTO face_clusters(label,created_at) VALUES('',?1)",
        params![now()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Store the assignment of faces to clusters. `manual` means the owner set it by hand.
/// A moved face has no proposal left (M26.17): the quarantine speaks about where the face is now,
/// and a membership the clustering or the owner decided is not a proposal.
pub fn save_face_assignments(
    conn: &mut Connection,
    assignments: &[(i64, Option<i64>)],
    manual: bool,
) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut st =
            tx.prepare("UPDATE faces SET cluster_id=?1, manual=?2, match_score=NULL WHERE id=?3")?;
        let mut drop_it = tx.prepare("DELETE FROM face_matches WHERE face_id=?1")?;
        for &(face_id, cluster_id) in assignments {
            st.execute(params![cluster_id, manual as i64, face_id])?;
            drop_it.execute([face_id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn label_face_cluster(conn: &Connection, cluster_id: i64, label: &str) -> Result<()> {
    conn.execute(
        "UPDATE face_clusters SET label=?1 WHERE id=?2",
        params![label.trim(), cluster_id],
    )?;
    Ok(())
}

/// Put several picked faces into one named group. If the name exists already,
/// the existing group is used; an empty name creates a new one without a label.
/// `manual=1` pins the decision through a rebuild.
pub fn group_faces(conn: &Connection, face_ids: &[i64], label: &str) -> Result<i64> {
    let label = label.trim();
    let existing: Option<i64> = if label.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT id FROM face_clusters WHERE label=?1 LIMIT 1",
            params![label],
            |r| r.get(0),
        )
        .optional()?
    };
    let cluster_id = match existing {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO face_clusters(label,created_at) VALUES(?1,?2)",
                params![label, now()],
            )?;
            conn.last_insert_rowid()
        }
    };
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare(
            "UPDATE faces SET cluster_id=?1, manual=1, match_score=NULL WHERE id=?2",
        )?;
        let mut drop_it = tx.prepare("DELETE FROM face_matches WHERE face_id=?1")?;
        for &id in face_ids {
            st.execute(params![cluster_id, id])?;
            drop_it.execute([id])?;
        }
    }
    tx.commit()?;
    delete_empty_face_clusters(conn)?;
    Ok(cluster_id)
}

/// Split one face into a new group: a false merge is undone with one action.
pub fn split_face(conn: &mut Connection, face_id: i64) -> Result<i64> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO face_clusters(label,created_at) VALUES('',?1)",
        params![now()],
    )?;
    let cluster_id = tx.last_insert_rowid();
    let n = tx.execute(
        "UPDATE faces SET cluster_id=?1, manual=1, match_score=NULL WHERE id=?2",
        params![cluster_id, face_id],
    )?;
    tx.execute("DELETE FROM face_matches WHERE face_id=?1", [face_id])?;
    if n == 0 {
        anyhow::bail!("no such face: {face_id}");
    }
    tx.commit()?;
    Ok(cluster_id)
}

/// What one split action did (M26.10): how many faces went into their own new groups and how
/// many ids were not in the database.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SplitOutcome {
    pub moved: usize,
    pub missing: usize,
}

/// Move every picked face into a group of its own, in one transaction (M26.10). A face that is
/// not in the database is counted and does not abort the others; the empty group is not left
/// behind. `manual=1` pins each decision through a rebuild, exactly as the single split does.
pub fn split_faces(conn: &mut Connection, face_ids: &[i64]) -> Result<SplitOutcome> {
    let tx = conn.transaction()?;
    let mut out = SplitOutcome::default();
    for &face_id in face_ids {
        tx.execute(
            "INSERT INTO face_clusters(label,created_at) VALUES('',?1)",
            params![now()],
        )?;
        let cluster_id = tx.last_insert_rowid();
        let n = tx.execute(
            "UPDATE faces SET cluster_id=?1, manual=1, match_score=NULL WHERE id=?2",
            params![cluster_id, face_id],
        )?;
        tx.execute("DELETE FROM face_matches WHERE face_id=?1", [face_id])?;
        if n == 0 {
            out.missing += 1;
            tx.execute("DELETE FROM face_clusters WHERE id=?1", params![cluster_id])?;
        } else {
            out.moved += 1;
        }
    }
    tx.commit()?;
    Ok(out)
}

/// Remove clusters without faces. A named cluster stays even while it is empty. A proposal to a
/// group that is gone goes with it (M26.17): nothing waits for a person who is no longer there.
pub fn delete_empty_face_clusters(conn: &Connection) -> Result<usize> {
    conn.execute(
        "DELETE FROM face_matches WHERE cluster_id NOT IN (SELECT id FROM face_clusters)",
        [],
    )?;
    Ok(conn.execute(
        "DELETE FROM face_clusters
         WHERE label='' AND id NOT IN (
            SELECT DISTINCT cluster_id FROM faces WHERE cluster_id IS NOT NULL
         )",
        [],
    )?)
}

/// The plan of the face pass: `(photos in all, not scanned yet)`.
pub fn face_scan_plan(conn: &Connection) -> Result<(usize, usize)> {
    let images: i64 = conn.query_row("SELECT COUNT(*) FROM files WHERE kind='image'", [], |r| {
        r.get(0)
    })?;
    let todo: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files f LEFT JOIN face_scans s ON s.file_id=f.id
         WHERE f.kind='image' AND COALESCE(s.sha256,'') != COALESCE(f.sha256,'')",
        [],
        |r| r.get(0),
    )?;
    Ok((images as usize, todo as usize))
}

/// `(faces, photos with faces, clusters)` — for the panel and the report.
pub fn face_summary(conn: &Connection) -> Result<(i64, i64, i64)> {
    let faces: i64 = conn.query_row("SELECT COUNT(*) FROM faces", [], |r| r.get(0))?;
    let photos: i64 = conn.query_row("SELECT COUNT(DISTINCT file_id) FROM faces", [], |r| {
        r.get(0)
    })?;
    let clusters: i64 = conn.query_row(
        "SELECT COUNT(*) FROM face_clusters WHERE id IN (SELECT cluster_id FROM faces WHERE cluster_id IS NOT NULL)",
        [],
        |r| r.get(0),
    )?;
    Ok((faces, photos, clusters))
}
