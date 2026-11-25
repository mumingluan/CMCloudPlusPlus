use rusqlite::{params, Connection};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq)]
pub enum BatchAction {
    Pause,
    Resume,
    Cancel,
}
impl BatchAction {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pause" => Ok(Self::Pause),
            "resume" => Ok(Self::Resume),
            "cancel" => Ok(Self::Cancel),
            _ => Err("不支持的批量传输操作".into()),
        }
    }
}

pub struct BatchTarget {
    pub id: String,
    pub path: String,
    pub download: bool,
}

pub fn apply(
    db: &mut Connection,
    action: BatchAction,
    direction: Option<&str>,
    account: Option<&str>,
    active: &HashSet<String>,
) -> Result<Vec<BatchTarget>, String> {
    let direction = match direction {
        None | Some("all") => None,
        Some(value @ ("upload" | "download")) => Some(value),
        _ => return Err("传输方向无效".into()),
    };
    if action == BatchAction::Resume && account.is_none() {
        return Err("请先连接云盘账号".into());
    }
    let eligible = match action {
        BatchAction::Pause => "status IN ('running','queued')",
        BatchAction::Resume => "status IN ('paused','failed')",
        BatchAction::Cancel => "status NOT IN ('completed','cancelled')",
    };
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let targets = {
        let mut query = tx.prepare(&format!(
            "SELECT id,local_path,direction FROM transfers WHERE {eligible} AND (?1 IS NULL OR direction=?1) AND (?2 IS NULL OR account_scope=?2)"
        )).map_err(|e| e.to_string())?;
        let rows = query
            .query_map(params![direction, account], |row| {
                Ok(BatchTarget {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    download: row.get::<_, String>(2)? == "download",
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let status = match action {
        BatchAction::Pause => "paused",
        BatchAction::Resume => "queued",
        BatchAction::Cancel => "cancelled",
    };
    for target in &targets {
        if action == BatchAction::Resume && active.contains(&target.id) {
            continue;
        }
        tx.execute("UPDATE transfers SET status=?1,error=NULL,attempts=CASE WHEN ?1='queued' THEN 0 ELSE attempts END,updated_at=?2 WHERE id=?3",
            params![status, chrono::Utc::now().timestamp_millis(), target.id]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE transfers(id TEXT PRIMARY KEY,local_path TEXT,direction TEXT,account_scope TEXT,status TEXT,error TEXT,attempts INTEGER,updated_at INTEGER);").unwrap();
        db
    }
    fn insert(db: &Connection, id: &str, direction: &str, account: &str, status: &str) {
        db.execute(
            "INSERT INTO transfers VALUES(?1,'path',?2,?3,?4,'error',3,0)",
            params![id, direction, account, status],
        )
        .unwrap();
    }
    fn status(db: &Connection, id: &str) -> String {
        db.query_row("SELECT status FROM transfers WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap()
    }
    #[test]
    fn pause_upload_covers_all_pages_without_affecting_downloads_or_finished_history() {
        let mut db = database();
        for n in 0..450 {
            insert(
                &db,
                &n.to_string(),
                "upload",
                "a",
                if n % 2 == 0 { "running" } else { "queued" },
            );
        }
        insert(&db, "download", "download", "a", "running");
        insert(&db, "done", "upload", "a", "completed");
        insert(&db, "failed", "upload", "a", "failed");
        let targets = apply(
            &mut db,
            BatchAction::Pause,
            Some("upload"),
            None,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(targets.len(), 450);
        assert_eq!(status(&db, "449"), "paused");
        assert_eq!(status(&db, "download"), "running");
        assert_eq!(status(&db, "done"), "completed");
        assert_eq!(status(&db, "failed"), "failed");
    }
    #[test]
    fn resume_is_account_and_direction_scoped_and_defers_unwinding_workers() {
        let mut db = database();
        for (id, direction, account, state) in [
            ("paused", "upload", "a", "paused"),
            ("failed", "upload", "a", "failed"),
            ("stopping", "upload", "a", "paused"),
            ("download", "download", "a", "paused"),
            ("other", "upload", "b", "paused"),
            ("cancelled", "upload", "a", "cancelled"),
        ] {
            insert(&db, id, direction, account, state);
        }
        let active = HashSet::from(["stopping".to_owned()]);
        assert_eq!(
            apply(
                &mut db,
                BatchAction::Resume,
                Some("upload"),
                Some("a"),
                &active
            )
            .unwrap()
            .len(),
            3
        );
        assert_eq!(status(&db, "paused"), "queued");
        assert_eq!(status(&db, "failed"), "queued");
        for id in ["stopping", "download", "other"] {
            assert_eq!(status(&db, id), "paused");
        }
        assert_eq!(status(&db, "cancelled"), "cancelled");
    }
    #[test]
    fn cancel_includes_paused_and_failed_but_preserves_finished_files_and_direction() {
        let mut db = database();
        for state in [
            "running",
            "queued",
            "paused",
            "failed",
            "completed",
            "cancelled",
        ] {
            insert(&db, state, "download", "a", state);
        }
        insert(&db, "upload", "upload", "a", "running");
        assert_eq!(
            apply(
                &mut db,
                BatchAction::Cancel,
                Some("download"),
                None,
                &HashSet::new()
            )
            .unwrap()
            .len(),
            4
        );
        for id in ["running", "queued", "paused", "failed"] {
            assert_eq!(status(&db, id), "cancelled");
        }
        assert_eq!(status(&db, "completed"), "completed");
        assert_eq!(status(&db, "upload"), "running");
        assert!(apply(
            &mut db,
            BatchAction::Cancel,
            Some("invalid"),
            None,
            &HashSet::new()
        )
        .is_err());
    }
}
