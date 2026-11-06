use crate::{
    cloud::{CloudClient, CloudFile, UploadPlan},
    store::Store,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "android")]
use tauri_plugin_document_picker::{DocumentPickerExt, ForegroundStatus};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::{Notify, RwLock, Semaphore},
};
use tokio_util::io::ReaderStream;

const MAX_CONCURRENCY: usize = 8;
const MAX_BATCH: usize = 10_000;
const MAX_QUEUE: usize = 100_000;
const IO_BUFFER: usize = 1024 * 1024;
const JOB_COLUMNS: &str = "id,direction,remote_json,local_path,name,total_bytes,completed_bytes,status,error,created_at,updated_at,attempts";
// Uploads may reuse a source in another account. Downloads must reserve their
// local destination globally, including against an upload still reading it.
const PATH_CONFLICT_QUERY: &str = "SELECT 1 FROM transfers WHERE local_path=?1 AND status NOT IN ('completed','cancelled') AND (?2='download' OR direction='download' OR account_scope=?3) LIMIT 1";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Download,
    Upload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSpec {
    pub direction: TransferDirection,
    /// The source file for downloads, destination folder for uploads.
    pub remote: CloudFile,
    pub local_path: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferJob {
    pub id: String,
    pub direction: TransferDirection,
    pub remote: CloudFile,
    pub local_path: String,
    pub name: String,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub attempts: u32,
    /// Runtime sample; resets after recovery or restart.
    pub speed: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgress {
    id: String,
    completed_bytes: u64,
    total_bytes: u64,
    speed: f64,
    phase: String,
}

#[derive(Serialize, Deserialize)]
struct UploadJournal {
    plan: UploadPlan,
    finished_parts: u64,
    source_size: u64,
    source_modified: Option<u64>,
}

#[derive(Serialize, Deserialize, Default)]
struct DownloadJournal {
    validator: Option<String>,
}

struct Control {
    action: AtomicU8,
    changed: Notify,
}
impl Control {
    fn new() -> Self {
        Self {
            action: AtomicU8::new(0),
            changed: Notify::new(),
        }
    }
    fn check(&self) -> Result<(), String> {
        match self.action.load(Ordering::Acquire) {
            1 => Err("已暂停".into()),
            2 => Err("已取消".into()),
            _ => Ok(()),
        }
    }
    fn set(&self, action: u8) {
        self.action.store(action, Ordering::Release);
        self.changed.notify_one();
    }
}

pub struct TransferManager {
    store: Arc<Store>,
    app: AppHandle,
    client: RwLock<Option<(CloudClient, String)>>,
    http: reqwest::Client,
    active: Mutex<HashMap<String, Arc<Control>>>,
    changed: Notify,
    #[cfg(target_os = "android")]
    foreground_changed: Notify,
    started: AtomicBool,
    concurrency: AtomicUsize,
    slots: Arc<Semaphore>,
}

impl TransferManager {
    pub fn new(store: Arc<Store>, app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            store,
            app,
            client: RwLock::new(None),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(90))
                .pool_idle_timeout(Duration::from_secs(60))
                .no_gzip()
                .build()
                .expect("TLS client initialization"),
            active: Mutex::new(HashMap::new()),
            changed: Notify::new(),
            #[cfg(target_os = "android")]
            foreground_changed: Notify::new(),
            started: AtomicBool::new(false),
            concurrency: AtomicUsize::new(4),
            slots: Arc::new(Semaphore::new(MAX_CONCURRENCY)),
        })
    }

    pub async fn set_client(&self, client: Option<CloudClient>) {
        let scope = if let Some(client) = &client {
            let cfg = client.account_config().await;
            let account = crate::auth::account_from_authorization(&cfg.authorization)
                .unwrap_or(cfg.username.clone());
            hex::encode(Sha256::digest(format!(
                "{account}\0{}\0{}\0{}",
                cfg.space, cfg.cloud_id, cfg.link_id
            )))
        } else {
            String::new()
        };
        // Switching sessions must stop existing requests before the next account runs.
        let mut session = self.client.write().await;
        if session.as_ref().map(|(_, existing)| existing.as_str()) != Some(scope.as_str()) {
            if let Ok(active) = self.active.lock() {
                for control in active.values() {
                    control.set(1);
                }
            }
            if let Ok(db) = self.store.db.lock() {
                let _ = db.execute("UPDATE transfers SET status='paused' WHERE status IN ('running','queued') AND account_scope != ?", [&scope]);
            }
        }
        *session = client.map(|client| (client, scope));
        self.notify_workers();
    }

    pub fn set_concurrency(&self, value: usize) -> Result<(), String> {
        if !(1..=MAX_CONCURRENCY).contains(&value) {
            return Err("并发数应为 1–8".into());
        }
        self.concurrency.store(value, Ordering::Release);
        self.notify_workers();
        Ok(())
    }
    pub fn concurrency(&self) -> usize {
        self.concurrency.load(Ordering::Acquire)
    }

    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            manager.dispatch().await;
        });
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            manager.cleanup_completed_parts().await;
        });
        #[cfg(target_os = "android")]
        {
            let manager = self.clone();
            tauri::async_runtime::spawn(async move {
                manager.foreground_heartbeat().await;
            });
        }
    }

    /// Preparation opens files without reading their contents; inserts occur in one transaction.
    pub async fn enqueue(&self, specs: Vec<TransferSpec>) -> Result<Vec<TransferJob>, String> {
        if specs.is_empty() {
            return Ok(Vec::new());
        }
        if specs.len() > MAX_BATCH {
            return Err("每批最多添加 10,000 项，请分批添加".into());
        }
        let scope = self
            .client
            .read()
            .await
            .as_ref()
            .map(|(_, scope)| scope.clone())
            .ok_or("请先连接云盘账号")?;
        let mut jobs = Vec::with_capacity(specs.len());
        let mut local_targets = HashSet::new();
        for spec in specs {
            validate_source(&spec)?;
            let (name, size) = if spec.direction == TransferDirection::Upload {
                let file = self.open_source(&spec.local_path).await?;
                let metadata = file.metadata().await.map_err(|e| e.to_string())?;
                if !metadata.is_file() {
                    return Err("上传源必须是文件".into());
                }
                (
                    spec.name.unwrap_or_else(|| source_name(&spec.local_path)),
                    metadata.len(),
                )
            } else {
                (
                    spec.name.unwrap_or_else(|| spec.remote.name.clone()),
                    spec.remote.size,
                )
            };
            if spec.direction == TransferDirection::Upload
                && (name.trim().is_empty()
                    || name == "."
                    || name == ".."
                    || name
                        .chars()
                        .any(|c| c == '/' || c == '\\' || c.is_control()))
            {
                return Err("云端文件名无效".into());
            }
            if size > i64::MAX as u64 {
                return Err("文件大小超出 SQLite 可表示范围".into());
            }
            if !local_targets.insert(spec.local_path.clone()) {
                return Err("同一批次含有重复的本地路径".into());
            }
            let time = now();
            jobs.push(TransferJob {
                id: uuid::Uuid::new_v4().to_string(),
                direction: spec.direction,
                remote: spec.remote,
                local_path: spec.local_path,
                name,
                total_bytes: size,
                completed_bytes: 0,
                status: "queued".into(),
                error: None,
                created_at: time,
                updated_at: time,
                attempts: 0,
                speed: 0.0,
            });
        }
        {
            let session = self.client.read().await;
            if session.as_ref().map(|(_, current)| current.as_str()) != Some(scope.as_str()) {
                return Err("账号或空间已经切换，请在当前空间重新添加任务".into());
            }
            let mut db = self
                .store
                .db
                .lock()
                .map_err(|_| "传输数据库不可用".to_string())?;
            let transaction = db.transaction().map_err(|e| e.to_string())?;
            let count: usize = transaction
                .query_row(
                    "SELECT COUNT(*) FROM transfers WHERE status NOT IN ('completed','cancelled')",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if count.saturating_add(jobs.len()) > MAX_QUEUE {
                return Err("待处理队列最多保留 100,000 项".into());
            }
            {
                let mut duplicate = transaction
                    .prepare(PATH_CONFLICT_QUERY)
                    .map_err(|e| e.to_string())?;
                let mut insert = transaction.prepare("INSERT INTO transfers(id,direction,remote_json,local_path,name,total_bytes,status,created_at,updated_at,account_scope) VALUES(?,?,?,?,?,?,'queued',?,?,?)").map_err(|e| e.to_string())?;
                for job in &jobs {
                    if duplicate
                        .query_row(
                            params![&job.local_path, direction_str(job.direction), &scope],
                            |_| Ok(()),
                        )
                        .optional()
                        .map_err(|e| e.to_string())?
                        .is_some()
                    {
                        return Err(format!("路径已有未完成任务：{}", job.name));
                    }
                    insert
                        .execute(params![
                            job.id,
                            direction_str(job.direction),
                            serde_json::to_string(&job.remote).map_err(|e| e.to_string())?,
                            job.local_path,
                            job.name,
                            job.total_bytes,
                            job.created_at,
                            job.updated_at,
                            scope,
                        ])
                        .map_err(|e| e.to_string())?;
                }
            }
            transaction.commit().map_err(|e| e.to_string())?;
        }
        self.notify_workers();
        let _ = self
            .app
            .emit("transfer-changed", serde_json::json!({"count":jobs.len()}));
        Ok(jobs)
    }

    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<TransferJob>, String> {
        self.list_filtered(limit, offset, "all")
    }

    pub fn list_filtered(
        &self,
        limit: usize,
        offset: usize,
        filter: &str,
    ) -> Result<Vec<TransferJob>, String> {
        let db = self
            .store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?;
        let (predicate,order)=match filter {
            "active"=>("WHERE status NOT IN ('completed','cancelled')","CASE status WHEN 'running' THEN 0 WHEN 'failed' THEN 1 WHEN 'paused' THEN 2 ELSE 3 END,created_at DESC,rowid DESC"),
            "completed"=>("WHERE status IN ('completed','cancelled')","created_at DESC,rowid DESC"),
            "all"=>("","created_at DESC,rowid DESC"),
            _=>return Err("不支持的传输筛选条件".into()),
        };
        let mut query = db
            .prepare(&format!(
                "SELECT {JOB_COLUMNS} FROM transfers {predicate} ORDER BY {order} LIMIT ? OFFSET ?"
            ))
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map(params![limit.clamp(1, 500), offset], read_job)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn pause(&self, id: &str) -> Result<(), String> {
        self.control(id, "paused", 1)
    }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        self.control(id, "cancelled", 2)?;
        if !self
            .active
            .lock()
            .map_err(|_| "传输状态不可用".to_string())?
            .contains_key(id)
        {
            let db = self
                .store
                .db
                .lock()
                .map_err(|_| "传输数据库不可用".to_string())?;
            let (path, direction): (String, String) = db
                .query_row(
                    "SELECT local_path,direction FROM transfers WHERE id=?",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            if direction == "download" {
                let _ = std::fs::remove_file(part_path(&path, id));
            }
        }
        Ok(())
    }
    pub fn resume(&self, id: &str) -> Result<(), String> {
        if self
            .active
            .lock()
            .map_err(|_| "传输状态不可用".to_string())?
            .contains_key(id)
        {
            return Err("任务正在停止，请稍后继续".into());
        }
        let scope = self
            .client
            .try_read()
            .map_err(|_| "账号正在切换，请稍后重试".to_string())?
            .as_ref()
            .map(|(_, scope)| scope.clone())
            .ok_or("请先连接云盘账号")?;
        let count = self.store.db.lock().map_err(|_| "传输数据库不可用".to_string())?
            .execute("UPDATE transfers SET status='queued',error=NULL,attempts=0,updated_at=? WHERE id=? AND account_scope=? AND status IN ('paused','failed')", params![now(), id, scope]).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("请连接任务所属账号和空间，再继续已暂停或失败的任务".into());
        }
        self.notify_workers();
        self.emit_changed(id);
        Ok(())
    }
    pub fn retry(&self, id: &str) -> Result<(), String> {
        self.resume(id)
    }
    pub fn remove_finished(&self) -> Result<usize, String> {
        self.store.clear_completed()
    }

    async fn cleanup_completed_parts(&self) {
        let mut offset = 0usize;
        loop {
            let paths: Vec<(String, String)> = {
                let Ok(db) = self.store.db.lock() else {
                    return;
                };
                let Ok(mut query)=db.prepare("SELECT id,local_path FROM transfers WHERE direction='download' AND status='completed' ORDER BY rowid LIMIT 500 OFFSET ?") else { return; };
                let Ok(rows) = query.query_map([offset], |row| Ok((row.get(0)?, row.get(1)?)))
                else {
                    return;
                };
                let Ok(paths) = rows.collect::<Result<Vec<_>, _>>() else {
                    return;
                };
                paths
            };
            if paths.is_empty() {
                return;
            }
            offset += paths.len();
            for (id, path) in paths {
                let temporary = part_path(&path, &id);
                if same_file(&temporary, Path::new(&path)) {
                    let _ = fs::remove_file(temporary).await;
                }
            }
        }
    }

    fn notify_workers(&self) {
        self.changed.notify_one();
        #[cfg(target_os = "android")]
        self.foreground_changed.notify_one();
    }

    #[cfg(target_os = "android")]
    async fn foreground_heartbeat(&self) {
        let mut last_attempt: Option<Instant> = None;
        loop {
            let wake = self.foreground_changed.notified();
            let running = self
                .active
                .lock()
                .map(|jobs| !jobs.is_empty())
                .unwrap_or(false);
            let scope = self
                .client
                .read()
                .await
                .as_ref()
                .map(|(_, scope)| scope.clone());
            let queued = scope.is_some_and(|scope| self.store.db.lock().ok().and_then(|db| db.query_row("SELECT EXISTS(SELECT 1 FROM transfers WHERE account_scope=? AND status='queued' LIMIT 1)",[scope],|row|row.get::<_,bool>(0)).ok()).unwrap_or(false));
            if running || queued {
                if last_attempt.is_none_or(|attempt| attempt.elapsed() >= Duration::from_secs(30)) {
                    let result = self.app.document_picker().begin_transfers().await;
                    let status = result.unwrap_or_else(|error| ForegroundStatus {
                        supported: true,
                        active: false,
                        notification_permission: false,
                        reason: Some(redact_error(&error)),
                    });
                    let _ = self.app.emit("foreground-status", status);
                    last_attempt = Some(Instant::now());
                }
            } else if last_attempt.take().is_some() {
                let end = self.app.document_picker().end_transfers().await;
                let mut status = self
                    .app
                    .document_picker()
                    .foreground_status()
                    .await
                    .unwrap_or(ForegroundStatus {
                        supported: true,
                        active: false,
                        notification_permission: false,
                        reason: None,
                    });
                if let Err(error) = end {
                    status.reason = Some(redact_error(&error));
                }
                let _ = self.app.emit("foreground-status", status);
            }
            let next = last_attempt
                .map(|attempt| Duration::from_secs(30).saturating_sub(attempt.elapsed()))
                .unwrap_or(Duration::from_secs(30))
                .max(Duration::from_millis(100));
            tokio::select! { _ = wake => {}, _ = tokio::time::sleep(next) => {} }
        }
    }

    fn control(&self, id: &str, status: &str, action: u8) -> Result<(), String> {
        let count = self.store.db.lock().map_err(|_| "传输数据库不可用".to_string())?
            .execute("UPDATE transfers SET status=?,error=NULL,updated_at=? WHERE id=? AND status NOT IN ('completed','cancelled')", params![status, now(), id]).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("任务不存在或已经结束".into());
        }
        if let Some(control) = self
            .active
            .lock()
            .map_err(|_| "传输状态不可用".to_string())?
            .get(id)
        {
            control.set(action);
        }
        self.emit_changed(id);
        self.notify_workers();
        Ok(())
    }

    async fn dispatch(self: Arc<Self>) {
        loop {
            let awake = self.changed.notified();
            let client = self.client.read().await.clone();
            let active_count = self
                .active
                .lock()
                .map(|m| m.len())
                .unwrap_or(MAX_CONCURRENCY);
            if let Some((client, scope)) =
                client.filter(|_| active_count < self.concurrency.load(Ordering::Acquire))
            {
                if let Ok(permit) = self.slots.clone().try_acquire_owned() {
                    match self.claim(&scope) {
                        Ok(Some(job)) => {
                            let control = Arc::new(Control::new());
                            if let Ok(mut active) = self.active.lock() {
                                active.insert(job.id.clone(), control.clone());
                            }
                            if self.status(&job.id).as_deref() != Ok("running") {
                                control.set(1);
                            }
                            let manager = self.clone();
                            tauri::async_runtime::spawn(async move {
                                let _permit = permit;
                                manager.clone().run(job, client, control).await;
                            });
                            continue;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let _ = self.app.emit("transfer-error", error);
                        }
                    }
                }
            }
            tokio::select! { _ = awake => {}, _ = tokio::time::sleep(Duration::from_secs(5)) => {} }
        }
    }

    fn claim(&self, scope: &str) -> Result<Option<TransferJob>, String> {
        let mut db = self
            .store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?;
        let transaction = db.transaction().map_err(|e| e.to_string())?;
        let query = format!("SELECT {JOB_COLUMNS} FROM transfers WHERE status='queued' AND account_scope=? ORDER BY created_at,rowid LIMIT 1");
        let mut job = transaction
            .query_row(&query, [scope], read_job)
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(job) = job.as_mut() {
            transaction
                .execute(
                    "UPDATE transfers SET status='running',updated_at=? WHERE id=?",
                    params![now(), job.id],
                )
                .map_err(|e| e.to_string())?;
            job.status = "running".into();
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(job)
    }

    async fn run(self: Arc<Self>, job: TransferJob, client: CloudClient, control: Arc<Control>) {
        self.emit_changed(&job.id);
        let mut outcome = Err("传输未开始".to_string());
        for attempt in 0..5u32 {
            if let Err(error) = control.check() {
                outcome = Err(error);
                break;
            }
            outcome = match job.direction {
                TransferDirection::Download => self.download(&job, &client, &control).await,
                TransferDirection::Upload => self.upload(&job, &client, &control).await,
            };
            if outcome.is_ok() || control.check().is_err() {
                break;
            }
            let error = outcome.as_ref().err().cloned().unwrap_or_default();
            if let Ok(db) = self.store.db.lock() {
                let _ = db.execute("UPDATE transfers SET attempts=?,error=?,updated_at=? WHERE id=? AND status='running'", params![attempt+1, redact_error(&error), now(), job.id]);
            }
            self.emit_changed(&job.id);
            if attempt < 4 {
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs((1u64 << attempt) + rand::random::<u64>() % 2)) => {}, _ = control.changed.notified() => {} }
            }
        }
        let (status, error) = match control.action.load(Ordering::Acquire) {
            1 => ("paused", None),
            2 => ("cancelled", None),
            _ => match outcome {
                Ok(()) => ("completed", None),
                Err(error) => ("failed", Some(redact_error(&error))),
            },
        };
        let committed = if let Ok(db) = self.store.db.lock() {
            db.execute("UPDATE transfers SET status=?,error=?,updated_at=? WHERE id=? AND status='running'", params![status, error, now(), job.id]).unwrap_or(0) > 0
        } else {
            false
        };
        if job.direction == TransferDirection::Download
            && (control.action.load(Ordering::Acquire) == 2 || (status == "completed" && committed))
        {
            let _ = fs::remove_file(part_path(&job.local_path, &job.id)).await;
        }
        if job.direction == TransferDirection::Upload && status == "completed" && committed {
            let cfg = client.account_config().await;
            let file = self
                .journal(&job.id)
                .ok()
                .flatten()
                .and_then(|text| serde_json::from_str::<UploadJournal>(&text).ok())
                .filter(|journal| !journal.plan.file_id.is_empty())
                .map(|journal| CloudFile {
                    id: journal.plan.file_id,
                    name: journal.plan.name,
                    path: job.remote.id.clone(),
                    is_dir: false,
                    size: journal.plan.size,
                    modified_at: chrono::Utc::now().to_rfc3339(),
                    thumbnail: None,
                });
            let _ = self.app.emit(
                "directory-changed",
                serde_json::json!({
                    "accountId": cfg.id,
                    "space": cfg.space,
                    "cloudId": cfg.cloud_id,
                    "parentId": job.remote.id,
                    "file": file,
                }),
            );
        }
        if let Ok(mut active) = self.active.lock() {
            active.remove(&job.id);
        }
        self.emit_changed(&job.id);
        self.notify_workers();
    }

    async fn download(
        &self,
        job: &TransferJob,
        client: &CloudClient,
        control: &Control,
    ) -> Result<(), String> {
        control.check()?;
        let destination = PathBuf::from(&job.local_path);
        let temporary = part_path(&job.local_path, &job.id);
        if destination.exists() {
            let durable = self.saved_completed(&job.id)?;
            if same_file(&temporary, &destination)
                && fs::metadata(&destination)
                    .await
                    .map_err(|e| e.to_string())?
                    .len()
                    == durable
                && (job.total_bytes == 0 || durable == job.total_bytes)
            {
                self.progress(job, durable, durable, 0.0, "completed");
                return Ok(());
            }
            return Err("目标文件已经存在，请更改保存位置".into());
        }
        let parent = destination.parent().ok_or("保存路径无效")?;
        fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&temporary)
            .await
            .map_err(|e| e.to_string())?;
        let partial_length = file.metadata().await.map_err(|e| e.to_string())?.len();
        // Only bytes synced before a persisted checkpoint are trusted after a crash.
        // Data written since then may have reached the page cache but not the disk.
        let durable = self.saved_completed(&job.id)?;
        let mut offset = partial_length.min(durable);
        if partial_length != offset {
            file.set_len(offset).await.map_err(|e| e.to_string())?;
        }
        if job.total_bytes > 0 && offset > job.total_bytes {
            return Err("断点文件大于云端文件，请取消后重新添加".into());
        }
        ensure_download_space(parent, job.total_bytes.saturating_sub(offset))?;
        let previous: DownloadJournal = self
            .journal(&job.id)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let link = client.link(&job.remote).await?;
        control.check()?;
        let mut request = self
            .http
            .get(link)
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
            if let Some(value) = &previous.validator {
                request = request.header(reqwest::header::IF_RANGE, value);
            }
        }
        let response = tokio::select! { result = request.send() => result.map_err(|e| e.without_url().to_string())?, _ = control.changed.notified() => { control.check()?; return Err("请求已中止".into()); } };
        let status = response.status();
        if response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|encoding| encoding != "identity")
        {
            return Err("服务器返回了压缩编码，无法安全进行断点下载".into());
        }
        if status.as_u16() == 416 && job.total_bytes > 0 && offset == job.total_bytes {
            let declared = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|header| header.strip_prefix("bytes */"))
                .and_then(|total| total.parse::<u64>().ok());
            if declared == Some(offset) {
                control.check()?;
                file.sync_all().await.map_err(|e| e.to_string())?;
                drop(file);
                publish_download(&temporary, &destination).await?;
                self.checkpoint(&job.id, offset, offset)?;
                self.progress(job, offset, offset, 0.0, "completed");
                return Ok(());
            }
        }
        if !status.is_success() {
            return Err(format!("下载请求返回 HTTP {}", status.as_u16()));
        }
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok());
        let disposition =
            resume_disposition(status.as_u16(), offset, content_range, job.total_bytes)?;
        if disposition == ResumeDisposition::Restart {
            file.set_len(0).await.map_err(|e| e.to_string())?;
            offset = 0;
        }
        let total = if let Some(range) = content_range {
            parse_content_range(range)?.2
        } else {
            response.content_length().unwrap_or(job.total_bytes)
        };
        if job.total_bytes > 0 && total > 0 && total != job.total_bytes {
            return Err("云端文件大小已变化，请重新添加任务".into());
        }
        let validator = response
            .headers()
            .get(reqwest::header::ETAG)
            .filter(|value| !value.as_bytes().starts_with(b"W/"))
            .or_else(|| response.headers().get(reqwest::header::LAST_MODIFIED))
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if disposition == ResumeDisposition::Append && offset > 0 {
            if let (Some(old), Some(new)) = (&previous.validator, &validator) {
                if old != new {
                    return Err("云端文件版本已变化，请重新添加任务".into());
                }
            }
        }
        self.save_journal(&job.id, &DownloadJournal { validator })?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| e.to_string())?;
        self.checkpoint(&job.id, offset, total)?;
        let mut stream = response.bytes_stream();
        let mut completed = offset;
        let mut meter = Meter::new(offset);
        loop {
            control.check()?;
            let next = tokio::select! { next = stream.next() => next, _ = control.changed.notified() => { control.check()?; continue; } };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|e| e.without_url().to_string())?;
            if total > 0 && completed.saturating_add(chunk.len() as u64) > total {
                return Err("服务器返回的数据超过文件大小，已停止写入".into());
            }
            for piece in chunk.chunks(IO_BUFFER) {
                control.check()?;
                file.write_all(piece).await.map_err(|e| e.to_string())?;
                completed += piece.len() as u64;
            }
            if meter.ready() {
                self.progress(
                    job,
                    completed,
                    total,
                    meter.sample(completed),
                    "transferring",
                );
                if meter.checkpoint_ready() {
                    file.sync_data().await.map_err(|e| e.to_string())?;
                    self.checkpoint(&job.id, completed, total)?;
                }
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        if total > 0 && completed != total {
            return Err(format!("下载中断，已保存断点（{completed}/{total} 字节）"));
        }
        control.check()?;
        self.checkpoint(&job.id, completed, completed)?;
        drop(file);
        publish_download(&temporary, &destination).await?;
        self.checkpoint(&job.id, completed, completed)?;
        self.progress(job, completed, completed, 0.0, "completed");
        Ok(())
    }

    async fn upload(
        &self,
        job: &TransferJob,
        client: &CloudClient,
        control: &Arc<Control>,
    ) -> Result<(), String> {
        let mut file = self.open_source(&job.local_path).await?;
        let metadata = file.metadata().await.map_err(|e| e.to_string())?;
        let size = metadata.len();
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64);
        if size != job.total_bytes {
            return Err("本地文件大小已变化，请重新添加任务".into());
        }
        let hash = {
            let mut hash = Sha256::new();
            let mut buffer = vec![0u8; IO_BUFFER];
            let mut hashed = 0u64;
            let mut meter = Meter::new(0);
            loop {
                control.check()?;
                let read = file.read(&mut buffer).await.map_err(|e| e.to_string())?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
                hashed += read as u64;
                if meter.ready() {
                    self.progress(job, hashed, size, meter.sample(hashed), "hashing");
                }
            }
            if hashed != size {
                return Err("计算哈希时本地文件发生变化".into());
            }
            hex::encode(hash.finalize()).to_uppercase()
        };
        control.check()?;
        let hash_metadata = file.metadata().await.map_err(|e| e.to_string())?;
        if hash_metadata.len() != size || metadata_stamp(&hash_metadata) != modified {
            return Err("计算哈希时本地文件发生变化，请重新添加任务".into());
        }
        let previous: Option<UploadJournal> = self
            .journal(&job.id)?
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            // Recreate obsolete streaming sessions through the current upload protocol.
            .filter(|value| value["plan"]["legacy"].as_bool() != Some(true))
            .and_then(|value| serde_json::from_value(value).ok());
        let mut first_part = None;
        let mut journal = match previous {
            Some(mut journal)
                if journal.source_size == size && journal.source_modified == modified =>
            {
                if !journal.plan.content_hash.is_empty() && journal.plan.content_hash != hash {
                    return Err("本地文件内容已变化，不能复用上传断点，请重新添加任务".into());
                }
                journal.plan.content_hash = hash.clone();
                self.save_journal(&job.id, &journal)?;
                journal
            }
            Some(_) => return Err("本地文件已修改，不能复用上传断点，请重新添加任务".into()),
            None => {
                let mut plan = client
                    .prepare_upload(&job.remote, &job.name, size, &hash)
                    .await?;
                first_part = plan.parts.first().cloned();
                // URLs are refreshed lazily. Keep checkpoints proportional to a task,
                // rather than serializing all previous parts at every boundary.
                plan.parts.clear();
                let journal = UploadJournal {
                    plan,
                    finished_parts: 0,
                    source_size: size,
                    source_modified: modified,
                };
                self.save_journal(&job.id, &journal)?;
                journal
            }
        };
        if journal.plan.rapid || journal.plan.existing {
            let current = self.open_source(&job.local_path).await?;
            let current_metadata = current.metadata().await.map_err(|e| e.to_string())?;
            if current_metadata.len() != size
                || metadata_stamp(&current_metadata) != journal.source_modified
            {
                return Err("准备秒传时本地文件发生变化，请重新添加任务".into());
            }
            control.check()?;
            self.checkpoint(&job.id, size, size)?;
            self.progress(job, size, size, 0.0, "completed");
            return Ok(());
        }
        if journal.plan.part_size == 0 {
            return Err("上传分片大小无效".into());
        }
        let part_count = size.div_ceil(journal.plan.part_size).max(1);
        if journal.finished_parts > part_count {
            return Err("上传断点数量无效".into());
        }
        let mut completed = journal
            .finished_parts
            .saturating_mul(journal.plan.part_size)
            .min(size);
        self.checkpoint(&job.id, completed, size)?;
        let mut start_number = journal.finished_parts + 1;
        while start_number <= part_count {
            control.check()?;
            // A batch of 100 URLs can expire long before its tail is reached on
            // a slow TB transfer. Refresh the next part just before sending it.
            let count = 1;
            let parts = if start_number == 1 && first_part.is_some() {
                vec![first_part.take().expect("checked initial upload part")]
            } else {
                client
                    .refresh_upload_parts(&journal.plan, start_number, count)
                    .await?
            };
            if parts.is_empty() {
                return Err("云端未返回上传分片地址，请重试".into());
            }
            for part in parts {
                if part.part_number <= journal.finished_parts {
                    continue;
                }
                if part.part_number == 0
                    || part.part_number > part_count
                    || part.offset.saturating_add(part.size) > size
                {
                    return Err("云端上传分片范围无效".into());
                }
                if part.part_number != journal.finished_parts + 1
                    || part.offset != completed
                    || part.size != size.saturating_sub(completed).min(journal.plan.part_size)
                {
                    return Err("上传分片顺序或大小不匹配".into());
                }
                control.check()?;
                let mut source = self.open_source(&job.local_path).await?;
                let source_metadata = source.metadata().await.map_err(|e| e.to_string())?;
                if source_metadata.len() != size
                    || metadata_stamp(&source_metadata) != journal.source_modified
                {
                    return Err("本地文件已变化，已停止传输并保留已确认的分片".into());
                }
                source
                    .seek(std::io::SeekFrom::Start(part.offset))
                    .await
                    .map_err(|e| e.to_string())?;
                let stream = ReaderStream::with_capacity(source.take(part.size), 64 * 1024);
                let manager_app = self.app.clone();
                let id = job.id.clone();
                let stop = control.clone();
                let baseline = completed;
                let mut sent = 0u64;
                let mut meter = Meter::new(0);
                let activity = Arc::new(AtomicU64::new(0));
                let activity_start = Instant::now();
                let stream_activity = activity.clone();
                let monitored = stream.map(move |chunk| {
                    if stop.check().is_err() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "上传已暂停或取消",
                        ));
                    }
                    if let Ok(chunk) = &chunk {
                        stream_activity
                            .store(activity_start.elapsed().as_secs(), Ordering::Relaxed);
                        sent += chunk.len() as u64;
                        if meter.ready() {
                            let _ = manager_app.emit(
                                "transfer-progress",
                                TransferProgress {
                                    id: id.clone(),
                                    completed_bytes: baseline + sent,
                                    total_bytes: size,
                                    speed: meter.sample(sent),
                                    phase: "transferring".into(),
                                },
                            );
                        }
                    }
                    chunk
                });
                let body = reqwest::Body::wrap_stream(monitored);
                let stalled = async {
                    loop {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        if activity_start
                            .elapsed()
                            .as_secs()
                            .saturating_sub(activity.load(Ordering::Relaxed))
                            >= 120
                        {
                            break;
                        }
                    }
                };
                tokio::select! {
                    result = client.upload_part(&part, body) => result?,
                    _ = stalled => return Err("上传超过 120 秒没有进展，保留已确认分片并自动重试".into()),
                    _ = control.changed.notified() => { control.check()?; return Err("上传已停止".into()); }
                }
                control.check()?;
                journal.finished_parts = part.part_number;
                completed += part.size;
                self.save_journal(&job.id, &journal)?;
                self.checkpoint(&job.id, completed, size)?;
                self.progress(job, completed, size, 0.0, "transferring");
            }
            start_number += count as u64;
        }
        if journal.finished_parts != part_count {
            return Err("上传分片不完整，保留断点等待重试".into());
        }
        let final_source = self.open_source(&job.local_path).await?;
        let final_metadata = final_source.metadata().await.map_err(|e| e.to_string())?;
        let final_modified = final_metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64);
        if final_metadata.len() != size || final_modified != journal.source_modified {
            return Err("传输中本地文件发生变化，已停止完成请求".into());
        }
        control.check()?;
        self.progress(job, completed, size, 0.0, "finalizing");
        client.complete_upload(&journal.plan).await?;
        self.checkpoint(&job.id, size, size)?;
        self.progress(job, size, size, 0.0, "completed");
        Ok(())
    }

    async fn open_source(&self, path: &str) -> Result<fs::File, String> {
        let app = self.app.clone();
        let path = path.to_owned();
        let file = tauri::async_runtime::spawn_blocking(move || {
            let path = FilePath::from_str(&path).map_err(|e| e.to_string())?;
            app.fs()
                .open(path, OpenOptions::new().read(true).clone())
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(fs::File::from_std(file))
    }

    fn status(&self, id: &str) -> Result<String, String> {
        self.store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?
            .query_row("SELECT status FROM transfers WHERE id=?", [id], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())
    }
    fn saved_completed(&self, id: &str) -> Result<u64, String> {
        self.store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?
            .query_row(
                "SELECT completed_bytes FROM transfers WHERE id=?",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }
    fn journal(&self, id: &str) -> Result<Option<String>, String> {
        let encrypted: Option<String> = self
            .store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?
            .query_row("SELECT upload_state FROM transfers WHERE id=?", [id], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        encrypted
            .map(|value| {
                let bytes = STANDARD
                    .decode(value)
                    .map_err(|_| "传输断点数据损坏".to_string())?;
                String::from_utf8(self.store.decrypt(&bytes)?)
                    .map_err(|_| "传输断点格式无效".to_string())
            })
            .transpose()
    }
    fn save_journal<T: Serialize>(&self, id: &str, journal: &T) -> Result<(), String> {
        let data = STANDARD.encode(
            self.store
                .encrypt(&serde_json::to_vec(journal).map_err(|e| e.to_string())?)?,
        );
        self.store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?
            .execute(
                "UPDATE transfers SET upload_state=? WHERE id=?",
                params![data, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn checkpoint(&self, id: &str, completed: u64, total: u64) -> Result<(), String> {
        self.store
            .db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?
            .execute(
                "UPDATE transfers SET completed_bytes=?,total_bytes=?,updated_at=? WHERE id=?",
                params![completed, total, now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn progress(&self, job: &TransferJob, completed: u64, total: u64, speed: f64, phase: &str) {
        let _ = self.app.emit(
            "transfer-progress",
            TransferProgress {
                id: job.id.clone(),
                completed_bytes: completed,
                total_bytes: total,
                speed,
                phase: phase.into(),
            },
        );
    }
    fn emit_changed(&self, id: &str) {
        let _ = self
            .app
            .emit("transfer-changed", serde_json::json!({ "id": id }));
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn metadata_stamp(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
}
fn direction_str(direction: TransferDirection) -> &'static str {
    if direction == TransferDirection::Download {
        "download"
    } else {
        "upload"
    }
}
fn read_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransferJob> {
    let json: String = row.get(2)?;
    let remote = serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let direction: String = row.get(1)?;
    Ok(TransferJob {
        id: row.get(0)?,
        direction: if direction == "upload" {
            TransferDirection::Upload
        } else {
            TransferDirection::Download
        },
        remote,
        local_path: row.get(3)?,
        name: row.get(4)?,
        total_bytes: row.get(5)?,
        completed_bytes: row.get(6)?,
        status: row.get(7)?,
        error: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        attempts: row.get(11)?,
        speed: 0.0,
    })
}

fn validate_source(spec: &TransferSpec) -> Result<(), String> {
    if spec.local_path.trim().is_empty() || spec.local_path.contains('\0') {
        return Err("本地路径无效".into());
    }
    let path = Path::new(&spec.local_path);
    if spec.direction == TransferDirection::Upload
        && (spec.local_path.starts_with("content://") || spec.local_path.starts_with("file://"))
    {
        return Ok(());
    }
    if !path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err("保存路径必须是绝对路径且不能含有 ..".into());
    }
    if spec.direction == TransferDirection::Download {
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            validate_filename(name)?;
        } else {
            return Err("保存路径必须包含文件名".into());
        }
    }
    Ok(())
}

pub fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("文件名包含不允许的字符或路径片段".into());
    }
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CLOCK$"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
    {
        return Err("文件名是 Windows 保留名称".into());
    }
    Ok(())
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        return "未命名文件".into();
    }
    if validate_filename(cleaned).is_err() {
        format!("_{cleaned}")
    } else {
        cleaned.into()
    }
}

fn source_name(source: &str) -> String {
    if source.starts_with("content://") || source.starts_with("file://") {
        let mut bytes = Vec::new();
        let source = source.as_bytes();
        let mut i = 0;
        while i < source.len() {
            if source[i] == b'%' && i + 2 < source.len() {
                if let Ok(value) =
                    u8::from_str_radix(std::str::from_utf8(&source[i + 1..i + 3]).unwrap_or(""), 16)
                {
                    bytes.push(value);
                    i += 3;
                    continue;
                }
            }
            bytes.push(source[i]);
            i += 1;
        }
        let decoded = String::from_utf8_lossy(&bytes);
        sanitize_filename(decoded.rsplit(['/', ':']).next().unwrap_or("上传文件"))
    } else {
        Path::new(source)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("上传文件")
            .into()
    }
}

fn part_path(destination: &str, id: &str) -> PathBuf {
    let path = Path::new(destination);
    path.with_file_name(format!(".cmcloud-{id}.part"))
}

fn ensure_download_space(directory: &Path, remaining: u64) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let directory: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0u64;
        if unsafe {
            GetDiskFreeSpaceExW(
                directory.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } != 0
            && remaining > available
        {
            return Err(format!(
                "磁盘空间不足：还需 {remaining} 字节，可用 {available} 字节"
            ));
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (directory, remaining);
    Ok(())
}

async fn publish_download(temporary: &Path, destination: &Path) -> Result<(), String> {
    // Same-directory hard link publishes completed bytes atomically without replacing
    // a destination. Windows FAT uses MoveFileEx without its replace-existing flag.
    match fs::hard_link(temporary, destination).await {
        // The link remains until SQLite confirms completion. If the process stops
        // between publication and commit, matching file identities recover it.
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err("目标文件已经存在".into())
        }
        Err(error) => {
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::ffi::OsStrExt;
                use windows_sys::Win32::Storage::FileSystem::{
                    MoveFileExW, MOVEFILE_WRITE_THROUGH,
                };
                let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
                let to: Vec<u16> = destination
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let _ = error;
            }
            #[cfg(not(target_os = "windows"))]
            return Err(format!(
                "当前文件系统无法安全发布完整下载，断点已保留：{error}"
            ));
        }
    }
    Ok(())
}

fn same_file(first: &Path, second: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let (Ok(first), Ok(second)) = (std::fs::File::open(first), std::fs::File::open(second))
        else {
            return false;
        };
        let mut a: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let mut b: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        unsafe {
            GetFileInformationByHandle(first.as_raw_handle() as _, &mut a) != 0
                && GetFileInformationByHandle(second.as_raw_handle() as _, &mut b) != 0
                && a.dwVolumeSerialNumber == b.dwVolumeSerialNumber
                && a.nFileIndexHigh == b.nFileIndexHigh
                && a.nFileIndexLow == b.nFileIndexLow
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (Ok(a), Ok(b)) = (std::fs::metadata(first), std::fs::metadata(second)) else {
            return false;
        };
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let _ = (first, second);
        false
    }
}

#[derive(Debug, PartialEq)]
enum ResumeDisposition {
    Append,
    Restart,
}
fn resume_disposition(
    status: u16,
    offset: u64,
    header: Option<&str>,
    expected_size: u64,
) -> Result<ResumeDisposition, String> {
    match status {
        200 => Ok(ResumeDisposition::Restart),
        206 => {
            let (start, end, total) =
                parse_content_range(header.ok_or("服务器返回 206 但缺少 Content-Range")?)?;
            if start != offset
                || end < start
                || end >= total
                || (expected_size > 0 && total != expected_size)
            {
                return Err("服务器断点范围不匹配，已停止以防文件损坏".into());
            }
            Ok(ResumeDisposition::Append)
        }
        _ => Err(format!("下载请求返回不支持的 HTTP {status}")),
    }
}
fn parse_content_range(value: &str) -> Result<(u64, u64, u64), String> {
    let value = value
        .strip_prefix("bytes ")
        .ok_or("Content-Range 单位无效")?;
    let (range, total) = value.split_once('/').ok_or("Content-Range 格式无效")?;
    let (start, end) = range.split_once('-').ok_or("Content-Range 范围无效")?;
    let parse = |value: &str| {
        value
            .parse::<u64>()
            .map_err(|_| "Content-Range 数值无效".to_string())
    };
    Ok((parse(start)?, parse(end)?, parse(total)?))
}

fn redact_error(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if [
        "authorization",
        "cookie",
        "password",
        "access_token",
        "refresh_token",
        "token=",
    ]
    .iter()
    .any(|key| lower.contains(key))
    {
        return "云端登录凭据失效或被拒绝，请重新连接账号后重试".into();
    }
    // Mask signed URLs even inside quotes, parentheses, or server JSON messages.
    let mut safe = String::new();
    let mut rest = error;
    loop {
        let start = match (rest.find("https://"), rest.find("http://")) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => {
                safe.push_str(rest);
                break;
            }
        };
        safe.push_str(&rest[..start]);
        safe.push_str("[云端地址]");
        let tail = &rest[start..];
        let end = tail
            .char_indices()
            .find(|(_, c)| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | '}'))
            .map(|(index, _)| index)
            .unwrap_or(tail.len());
        rest = &tail[end..];
    }
    safe.chars().take(500).collect()
}

struct Meter {
    last: Instant,
    last_bytes: u64,
    checkpoint: Instant,
}
impl Meter {
    fn new(bytes: u64) -> Self {
        Self {
            last: Instant::now(),
            last_bytes: bytes,
            checkpoint: Instant::now(),
        }
    }
    fn ready(&self) -> bool {
        self.last.elapsed() >= Duration::from_millis(300)
    }
    fn sample(&mut self, bytes: u64) -> f64 {
        let speed = bytes.saturating_sub(self.last_bytes) as f64
            / self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        self.last_bytes = bytes;
        speed
    }
    fn checkpoint_ready(&mut self) -> bool {
        if self.checkpoint.elapsed() >= Duration::from_secs(2) {
            self.checkpoint = Instant::now();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_switch_allows_shared_upload_sources_but_protects_download_targets() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE transfers(local_path TEXT,direction TEXT,status TEXT,account_scope TEXT);
            INSERT INTO transfers VALUES ('source','upload','paused','account-a'),('destination','download','paused','account-a'),('finished','upload','completed','account-a');").unwrap();
        let conflict = |path: &str, direction: &str, scope: &str| {
            db.query_row(PATH_CONFLICT_QUERY, params![path, direction, scope], |_| {
                Ok(())
            })
            .optional()
            .unwrap()
            .is_some()
        };
        assert!(conflict("source", "upload", "account-a"));
        assert!(!conflict("source", "upload", "account-b"));
        assert!(conflict("source", "download", "account-b"));
        assert!(conflict("destination", "upload", "account-b"));
        assert!(conflict("destination", "download", "account-b"));
        assert!(!conflict("finished", "upload", "account-a"));
    }
    #[test]
    fn server_ignoring_range_restarts_instead_of_appending() {
        assert_eq!(
            resume_disposition(200, 8192, None, 16384).unwrap(),
            ResumeDisposition::Restart
        );
    }
    #[test]
    fn resume_requires_matching_range_and_file_size() {
        assert_eq!(
            resume_disposition(206, 8192, Some("bytes 8192-16383/16384"), 16384).unwrap(),
            ResumeDisposition::Append
        );
        assert!(resume_disposition(206, 8192, Some("bytes 0-16383/16384"), 16384).is_err());
        assert!(resume_disposition(206, 8192, Some("bytes 8192-16383/32768"), 16384).is_err());
        assert!(resume_disposition(206, 8192, None, 16384).is_err());
    }
    #[test]
    fn tb_range_values_are_64_bit() {
        assert_eq!(
            parse_content_range("bytes 1099511627776-2199023255551/2199023255552").unwrap(),
            (1099511627776, 2199023255551, 2199023255552)
        );
    }
    #[test]
    fn cloud_filenames_cannot_escape_selected_directory() {
        for name in [
            "../credentials",
            "folder/file",
            "folder\\file",
            "CON.txt",
            "LPT1",
            "a:stream",
            "..",
            "name.",
        ] {
            assert!(validate_filename(name).is_err(), "{name}");
        }
        assert!(validate_filename("数据备份.zip").is_ok());
        assert_eq!(sanitize_filename("../a:b"), ".._a_b");
    }
    #[test]
    fn android_document_name_is_decoded() {
        assert_eq!(
            source_name("content://provider/document/primary%3ADownload%2F%E6%95%B0%E6%8D%AE.zip"),
            "数据.zip"
        );
    }
    #[tokio::test]
    async fn publish_refuses_overwrite_and_preserves_crash_recovery_identity() {
        let directory =
            std::env::temp_dir().join(format!("cmcloud-publish-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).await.unwrap();
        let partial = directory.join("source.part");
        let destination = directory.join("result.bin");
        let existing = directory.join("existing.bin");
        fs::write(&partial, b"completed-file-bytes").await.unwrap();
        publish_download(&partial, &destination).await.unwrap();
        assert!(same_file(&partial, &destination));
        fs::write(&existing, b"user-owned-file").await.unwrap();
        assert!(publish_download(&partial, &existing).await.is_err());
        assert_eq!(fs::read(existing).await.unwrap(), b"user-owned-file");
        fs::remove_dir_all(directory).await.unwrap();
    }
    #[test]
    fn signed_urls_and_credential_values_do_not_reach_transfer_errors() {
        let redacted = redact_error(
            "request failed (https://storage.example/upload?signature=private) HTTP 403",
        );
        assert!(!redacted.contains("private"));
        assert!(redacted.contains("HTTP 403"));
        assert!(!redact_error("Authorization: Bearer secret-value").contains("secret-value"));
    }
}
