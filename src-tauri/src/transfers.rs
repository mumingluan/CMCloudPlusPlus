use crate::{
    cloud::{CloudClient, CloudFile, UploadPlan},
    store::Store,
    transfer_activity::{ActivityJob, TransferActivity, TransferOverview},
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
use tauri::{AppHandle, Emitter, Manager};
use crate::repositories::queue::RepositoryTransfer;
#[path = "openlist_crypt_transfers.rs"]
mod crypt_transfers;
pub(crate) use crypt_transfers::{CryptTransfer, SpecialTransfer};
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
const JOB_COLUMNS: &str = "id,direction,remote_json,local_path,name,total_bytes,completed_bytes,status,error,created_at,updated_at,attempts,repository_state IS NOT NULL";
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
    #[serde(default)]
    pub repository: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publication: Option<DownloadPublication>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct DownloadPublication {
    device: u64,
    inode: u64,
    bytes: u64,
}

#[cfg(target_os = "android")]
fn download_identity(metadata: &std::fs::Metadata) -> DownloadPublication {
    use std::os::unix::fs::MetadataExt;
    DownloadPublication {
        device: metadata.dev(),
        inode: metadata.ino(),
        bytes: metadata.len(),
    }
}

pub(crate) struct Control {
    action: AtomicU8,
    resume_after_stop: AtomicBool,
    changed: Notify,
    cancellations:Mutex<Vec<Arc<AtomicBool>>>,
}
impl Control {
    fn new() -> Self {
        Self {
            action: AtomicU8::new(0),
            resume_after_stop: AtomicBool::new(false),
            changed: Notify::new(),
            cancellations:Mutex::new(vec![]),
        }
    }
    pub(crate) fn check(&self) -> Result<(), String> {
        match self.action.load(Ordering::Acquire) {
            1 => Err("已暂停".into()),
            2 => Err("已取消".into()),
            _ => Ok(()),
        }
    }
    fn set(&self, action: u8) {
        self.resume_after_stop.store(false, Ordering::Release);
        self.action.store(action, Ordering::Release);
        if let Ok(flags)=self.cancellations.lock(){for flag in flags.iter(){flag.store(true,Ordering::Release)}}
        self.changed.notify_one();
    }
    pub(crate) fn attach_cancel(&self,flag:Arc<AtomicBool>)->Result<(),String>{let mut flags=self.cancellations.lock().map_err(|_|"传输控制不可用")?;if self.action.load(Ordering::Acquire)!=0{flag.store(true,Ordering::Release)}flags.push(flag);Ok(())}
}

pub struct TransferManager {
    store: Arc<Store>,
    app: AppHandle,
    client: RwLock<Option<(CloudClient, String)>>,
    http: reqwest::Client,
    active: Mutex<HashMap<String, Arc<Control>>>,
    activity: Arc<Mutex<TransferActivity>>,
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
            activity: Arc::new(Mutex::new(TransferActivity::default())),
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
            if let Ok(mut activity) = self.activity.lock() {
                activity.clear();
            }
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
        let intents=vec![None;specs.len()];self.enqueue_with_repository(specs,intents).await
    }
    pub(crate) async fn enqueue_with_repository(&self,specs:Vec<TransferSpec>,intents:Vec<Option<RepositoryTransfer>>)->Result<Vec<TransferJob>,String>{
        let mapped = intents.into_iter().map(|i| i.map(SpecialTransfer::Repository)).collect();
        self.enqueue_with_special(specs, mapped).await
    }
    pub(crate) async fn enqueue_with_special(&self,specs:Vec<TransferSpec>,intents:Vec<Option<SpecialTransfer>>)->Result<Vec<TransferJob>,String>{
        if specs.len()!=intents.len(){return Err("加密或对象库传输批次无效".into())}
        if specs.is_empty() {
            return Ok(Vec::new());
        }
        if specs.len() > MAX_BATCH {
            return Err("每批最多添加 10,000 项，请分批添加".into());
        }
        #[cfg(target_os = "android")]
        if specs.iter().zip(&intents).any(|(spec, intent)| {
            spec.direction == TransferDirection::Download
                && intent.as_ref().and_then(SpecialTransfer::preview_path).is_none()
        }) {
            self.app.document_picker().ensure_shared_downloads().await?;
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
        for (spec,intent) in specs.into_iter().zip(&intents) {
            validate_source(&spec)?;
            let (name, size) = if let Some(intent)=intent {intent.queue_size_name(&spec)?} else if spec.direction == TransferDirection::Upload {
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
                repository:intent.is_some(),
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
        let sealed_intents=intents.iter().map(|intent|intent.as_ref().map(|v|serde_json::to_vec(v).map_err(|_|"对象库传输编码失败".to_string()).and_then(|raw|{if raw.len()>32<<20{return Err("对象库传输批次过大，请分批添加".into())}self.store.encrypt(&raw).map(|bytes|STANDARD.encode(bytes))})).transpose()).collect::<Result<Vec<_>,_>>()?;
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
                let mut insert = transaction.prepare("INSERT INTO transfers(id,direction,remote_json,local_path,name,total_bytes,status,created_at,updated_at,account_scope,repository_state,repository_preview) VALUES(?,?,?,?,?,?,'queued',?,?,?,?,?)").map_err(|e| e.to_string())?;
                for ((job,intent),original) in jobs.iter().zip(&sealed_intents).zip(&intents) {
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
                            intent,
                            original.as_ref().is_some_and(|intent|intent.preview_path().is_some()),
                        ])
                        .map_err(|e| e.to_string())?;
                }
            }
            transaction.commit().map_err(|e| e.to_string())?;
            if let Ok(mut activity) = self.activity.lock() {
                activity.observe(jobs.iter().map(activity_job).collect());
            }
        }
        self.notify_workers();
        let _ = self
            .app
            .emit("transfer-changed", serde_json::json!({"count":jobs.len()}));
        Ok(jobs)
    }

    pub(crate) fn get(&self,id:&str)->Result<TransferJob,String>{
        self.store.db.lock().map_err(|_|"传输数据库不可用")?.query_row(&format!("SELECT {JOB_COLUMNS} FROM transfers WHERE id=?"),[id],read_job).map_err(|_|"传输任务不存在".into())
    }
    pub(crate) fn is_active(&self,id:&str)->Result<bool,String>{Ok(self.active.lock().map_err(|_|"传输状态不可用")?.contains_key(id))}
    fn preview_output(&self,id:&str)->Result<Option<PathBuf>,String>{
        let encoded:Option<String>=self.store.db.lock().map_err(|_|"传输数据库不可用")?.query_row("SELECT repository_state FROM transfers WHERE id=?",[id],|row|row.get(0)).optional().map_err(|_|"预览状态不可用")?.flatten();
        let Some(encoded)=encoded else{return Ok(None)};if encoded.len()>48<<20{return Err("传输断点过大".into())}
        let raw=zeroize::Zeroizing::new(self.store.decrypt(&STANDARD.decode(encoded).map_err(|_|"预览断点已损坏")?)?);
        let intent:SpecialTransfer=serde_json::from_slice(&raw).map_err(|_|"预览断点已损坏")?;
        Ok(intent.preview_path().map(PathBuf::from))
    }
    pub(crate) fn discard_preview(&self,id:&str,root:&Path)->Result<(),String>{
        if self.is_active(id)?{return Err("预览仍在停止".into())}
        let Some(path)=self.preview_output(id)? else{return Ok(())};
        let relative=path.strip_prefix(root).map_err(|_|"预览缓存身份无效")?;
        let parts=relative.components().collect::<Vec<_>>();
        if parts.len()!=2||parts.iter().any(|part|!matches!(part,std::path::Component::Normal(_)))||uuid::Uuid::parse_str(&parts[0].as_os_str().to_string_lossy()).is_err(){return Err("预览缓存路径无效".into())}
        for ancestor in path.ancestors(){if let Ok(meta)=std::fs::symlink_metadata(ancestor){if crate::repositories::cache::reparse(&meta){return Err("预览缓存含链接".into())}}}
        if path.exists(){std::fs::remove_file(&path).map_err(|_|"预览缓存未能清除，请在下次启动时重试")?;}
        if let Some(parent)=path.parent(){if parent.exists(){std::fs::remove_dir(parent).map_err(|_|"预览临时目录未能回收")?;}}
        self.store.db.lock().map_err(|_|"传输数据库不可用")?.execute("DELETE FROM transfers WHERE id=?",[id]).map_err(|_|"预览任务未能回收")?;
        self.emit_changed(id);Ok(())
    }
    pub(crate) fn discard_previews_on_start(&self,root:&Path)->Result<(),String>{
        let ids={let db=self.store.db.lock().map_err(|_|"传输数据库不可用")?;let mut query=db.prepare("SELECT id FROM transfers WHERE repository_preview=1").map_err(|_|"预览列表不可用")?;let rows=query.query_map([],|row|row.get::<_,String>(0)).map_err(|_|"预览列表不可用")?.collect::<Result<Vec<_>,_>>().map_err(|_|"预览列表不可用")?;rows};
        for id in ids{self.discard_preview(&id,root)?;}Ok(())
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
        let (predicate, order) = transfer_filter(filter)?;
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

    pub fn pause_all(&self) -> Result<(), String> {
        let db = self.store.db.lock().map_err(|_| "传输数据库不可用")?;
        db.execute("UPDATE transfers SET status='paused',error=NULL,updated_at=? WHERE status IN ('running','queued')", [now()]).map_err(|e| e.to_string())?;
        if let Ok(active) = self.active.lock() {
            for control in active.values() {
                control.set(1);
            }
        }
        drop(db);
        self.notify_workers();
        self.emit_changed("");
        Ok(())
    }

    pub async fn resume_all(&self) -> Result<(), String> {
        let session = self.client.read().await;
        let scope = &session.as_ref().ok_or("请先连接云盘账号")?.1;
        let mut db = self.store.db.lock().map_err(|_| "传输数据库不可用")?;
        let active = self.active.lock().map_err(|_| "传输状态不可用")?;
        // A paused worker can still be unwinding an in-flight request. Resume it
        // only after its checkpoint and cleanup complete, never run the ID twice.
        for control in active.values() {
            if control.action.load(Ordering::Acquire) == 1 {
                control.resume_after_stop.store(true, Ordering::Release);
            }
        }
        resume_scope(&mut db, scope, &active.keys().cloned().collect())?;
        drop(active);
        drop(db);
        self.notify_workers();
        self.emit_changed("");
        Ok(())
    }

    pub async fn control_batch(
        &self,
        action: &str,
        direction: Option<&str>,
    ) -> Result<usize, String> {
        use crate::batch_control::{self, BatchAction};
        let action = BatchAction::parse(action)?;
        let session = self.client.read().await;
        let account = if action == BatchAction::Resume {
            Some(session.as_ref().ok_or("请先连接云盘账号")?.1.as_str())
        } else {
            None
        };
        #[cfg(target_os = "android")]
        let mut shared_cleanup = Vec::new();
        let (count, cleanup) = {
            let mut db = self.store.db.lock().map_err(|_| "传输数据库不可用")?;
            let active = self.active.lock().map_err(|_| "传输状态不可用")?;
            let targets = batch_control::apply(
                &mut db,
                action,
                direction,
                account,
                &active.keys().cloned().collect(),
            )?;
            let mut cleanup = Vec::new();
            for target in &targets {
                if let Some(control) = active.get(&target.id) {
                    match action {
                        BatchAction::Pause => control.set(1),
                        BatchAction::Cancel => control.set(2),
                        BatchAction::Resume => {
                            control.resume_after_stop.store(true, Ordering::Release)
                        }
                    }
                } else if action == BatchAction::Cancel && target.download {
                    cleanup.push(part_path(&target.path, &target.id));
                    #[cfg(target_os = "android")]
                    if let Ok(Some(uri)) = db.query_row(
                        "SELECT shared_download_uri FROM transfers WHERE id=?", [&target.id],
                        |row| row.get::<_, Option<String>>(0))
                    {
                        shared_cleanup.push((target.path.clone(), uri));
                    }
                }
            }
            if let Ok(mut activity) = self.activity.lock() {
                for target in &targets {
                    let status = match action {
                        BatchAction::Pause => "paused",
                        BatchAction::Cancel => "cancelled",
                        BatchAction::Resume if active.contains_key(&target.id) => "paused",
                        BatchAction::Resume => "queued",
                    };
                    activity.set_status(&target.id, status);
                }
            }
            (targets.len(), cleanup)
        };
        drop(session);
        let mut cleanup_errors = 0usize;
        for path in cleanup {
            if let Err(error) = fs::remove_file(path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    cleanup_errors += 1;
                }
            }
        }
        #[cfg(target_os = "android")]
        for (path, uri) in shared_cleanup {
            if discard_shared_stage(&self.app, &path, &uri).await.is_err() {
                cleanup_errors += 1;
            }
        }
        self.notify_workers();
        self.emit_changed("");
        if cleanup_errors > 0 {
            return Err(format!(
                "任务已取消，但有 {cleanup_errors} 个下载临时文件未能清理"
            ));
        }
        Ok(count)
    }

    pub async fn overview(&self) -> Result<TransferOverview, String> {
        let session = self.client.read().await;
        let Some((_, scope)) = session.as_ref() else {
            return Ok(TransferOverview::default());
        };
        let db = self.store.db.lock().map_err(|_| "传输数据库不可用")?;
        let mut query = db.prepare("SELECT id,direction,total_bytes,completed_bytes,status FROM transfers WHERE account_scope=? AND status NOT IN ('completed','cancelled')").map_err(|e| e.to_string())?;
        let jobs = query
            .query_map([scope], |row| {
                Ok(ActivityJob {
                    id: row.get(0)?,
                    upload: row.get::<_, String>(1)? == "upload",
                    total: row.get(2)?,
                    completed: row.get(3)?,
                    status: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut activity = self.activity.lock().map_err(|_| "传输统计不可用")?;
        activity.observe(jobs);
        Ok(activity.overview())
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
            let (path, direction, uri): (String, String, Option<String>) = db
                .query_row(
                    "SELECT local_path,direction,shared_download_uri FROM transfers WHERE id=?",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|e| e.to_string())?;
            #[cfg(not(target_os = "android"))]
            let _ = uri;
            if direction == "download" {
                let _ = std::fs::remove_file(part_path(&path, id));
                #[cfg(target_os = "android")]
                if let Some(uri) = uri {
                    let app = self.app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = discard_shared_stage(&app, &path, &uri).await {
                            eprintln!("下载暂存清理失败：{error}");
                        }
                    });
                }
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
    pub(crate) async fn quiesce_repositories(&self)->Result<(),String>{
        let ids={let db=self.store.db.lock().map_err(|_|"传输数据库不可用")?;let mut query=db.prepare("SELECT id FROM transfers WHERE repository_state IS NOT NULL AND status NOT IN ('completed','cancelled')").map_err(|_|"对象库传输列表不可用")?;let rows=query.query_map([],|row|row.get::<_,String>(0)).map_err(|_|"对象库传输列表不可用")?.collect::<Result<Vec<_>,_>>().map_err(|_|"对象库传输列表不可用")?;rows};
        for id in &ids{self.pause(id)?;}
        let deadline=Instant::now()+Duration::from_secs(120);
        loop{let active=self.active.lock().map_err(|_|"对象库传输控制不可用")?.keys().any(|id|ids.contains(id));if !active{return Ok(())}if Instant::now()>=deadline{return Err("对象库传输正在停止，请稍后再清除凭据".into())}tokio::time::sleep(Duration::from_millis(100)).await;}
    }
    pub fn remove_finished(&self) -> Result<usize, String> {
        let count = self.store.clear_completed()?;
        self.emit_changed("");
        Ok(count)
    }

    pub fn remove_completed(&self, id: &str) -> Result<(), String> {
        self.store.remove_completed(id)?;
        self.emit_changed(id);
        Ok(())
    }

    #[cfg(target_os = "android")]
    fn shared_download_info(&self, id: &str) -> Result<(Option<String>, bool), String> {
        self.store.db.lock().map_err(|_| "传输数据库不可用".to_string())?
            .query_row("SELECT shared_download_uri,repository_preview FROM transfers WHERE id=?", [id],
                |row| Ok((row.get(0)?, row.get(1)?))).map_err(|e| e.to_string())
    }

    #[cfg(target_os = "android")]
    async fn publish_shared_download(&self, job: &TransferJob) -> Result<Option<String>, String> {
        let (previous, preview) = self.shared_download_info(&job.id)?;
        if preview || Path::new(&job.local_path).is_dir() {
            return Ok(None);
        }
        let root = self.app.path().app_data_dir().map_err(|e| e.to_string())?.join("Downloads");
        let parent = Path::new(&job.local_path).parent().ok_or("下载路径无效")?;
        let relative = parent.strip_prefix(&root).map_err(|_| "下载暂存路径无效")?;
        let mut folders = relative.iter();
        if job.repository && relative.starts_with("AxuanStor") {
            folders.next();
            folders.next(); // The repository queue uses a private per-batch directory.
        }
        let folder = folders.map(|part| part.to_string_lossy()).collect::<Vec<_>>().join("/");
        let durable = self.saved_completed(&job.id)?;
        self.progress(job, durable, durable, 0.0, "publishing");
        let picker = self.app.document_picker();
        let uri = picker.prepare_shared_download(&job.local_path, &folder, previous.as_deref()).await?;
        let saved = self.store.db.lock().ok().and_then(|db| {
            db.execute("UPDATE transfers SET shared_download_uri=? WHERE id=?", params![uri, job.id]).ok()
        }) == Some(1);
        if !saved {
            if previous.as_deref() != Some(&uri) {
                let _ = picker.discard_shared_download(&uri).await;
            }
            return Err("无法保存公共下载记录".into());
        }
        picker.finish_shared_download(&job.local_path, &uri).await?;
        Ok(Some(uri))
    }

    #[cfg(target_os = "android")]
    fn cleanup_shared_stage(&self, id: &str, path: &str, uri: &str) {
        let Ok(root) = self.app.path().app_data_dir() else { return; };
        if !Path::new(path).starts_with(root.join("Downloads")) { return; }
        if let Ok(db) = self.store.db.lock() {
            let reserved = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM transfers WHERE local_path=? AND id<>? AND status NOT IN ('completed','cancelled'))",
                params![path, id], |row| row.get::<_, bool>(0)).unwrap_or(true);
            if !reserved {
                match std::fs::remove_file(path) {
                    Ok(()) => {},
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                    Err(_) => return,
                }
            }
            let _ = db.execute("UPDATE transfers SET local_path=? WHERE id=? AND local_path=? AND status='completed'",
                params![uri, id, path]);
        }
    }

    async fn cleanup_completed_parts(&self) {
        let mut offset = 0usize;
        loop {
            let paths: Vec<(String, String, Option<String>)> = {
                let Ok(db) = self.store.db.lock() else {
                    return;
                };
                let Ok(mut query)=db.prepare("SELECT id,local_path,shared_download_uri FROM transfers WHERE direction='download' AND status='completed' ORDER BY rowid LIMIT 500 OFFSET ?") else { return; };
                let Ok(rows) = query.query_map([offset], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
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
            for (id, path, uri) in paths {
                #[cfg(target_os = "android")]
                if let Some(uri) = &uri {
                    if path != *uri {
                        self.cleanup_shared_stage(&id, &path, uri);
                    }
                    continue;
                }
                #[cfg(not(target_os = "android"))]
                let _ = uri;
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
                if last_attempt.is_none_or(|attempt| attempt.elapsed() >= Duration::from_secs(10)) {
                    let overview = self.overview().await.unwrap_or_default();
                    let result = self
                        .app
                        .document_picker()
                        .begin_transfers_with_status(
                            serde_json::to_value(overview).unwrap_or_default(),
                        )
                        .await;
                    let status = result.unwrap_or_else(|error| ForegroundStatus {
                        supported: true,
                        active: false,
                        notification_permission: false,
                        user_stopped: false,
                        reason: Some(redact_error(&error)),
                    });
                    if status.user_stopped {
                        let _ = self.pause_all();
                    }
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
                        user_stopped: false,
                        reason: None,
                    });
                if let Err(error) = end {
                    status.reason = Some(redact_error(&error));
                }
                let _ = self.app.emit("foreground-status", status);
            }
            let next = last_attempt
                .map(|attempt| Duration::from_secs(10).saturating_sub(attempt.elapsed()))
                .unwrap_or(Duration::from_secs(10))
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
        if let Ok(mut activity) = self.activity.lock() {
            activity.set_status(id, status);
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
                            let status = self.status(&job.id);
                            if status.as_deref() == Ok("cancelled") {
                                control.set(2);
                            } else if status.as_deref() != Ok("running")
                                && control.action.load(Ordering::Acquire) == 0
                            {
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
        let mut outcome: Result<(), String>;
        let mut attempt = 0u32;
        #[cfg(target_os = "android")]
        let mut shared_uri = None;
        loop {
            if let Err(error) = control.check() {
                outcome = Err(error);
                break;
            }
            #[cfg(target_os = "android")]
            let already_staged = job.direction == TransferDirection::Download &&
                Path::new(&job.local_path).is_file() &&
                self.shared_download_info(&job.id).is_ok_and(|(uri, preview)| !preview &&
                    (uri.is_some() || (job.repository && self.saved_completed(&job.id).ok() == Some(job.total_bytes))));
            #[cfg(not(target_os = "android"))]
            let already_staged = false;
            outcome = if already_staged {
                Ok(())
            } else if job.repository {
                self.run_repository(&job,&client,&control).await
            } else {match job.direction {
                TransferDirection::Download => self.download(&job, &client, &control).await,
                TransferDirection::Upload => self.upload(&job, &client, &control).await,
            }};
            #[cfg(target_os = "android")]
            if outcome.is_ok() && job.direction == TransferDirection::Download {
                outcome = match control.check() {
                    Ok(()) => self.publish_shared_download(&job).await.map(|uri| shared_uri = uri),
                    Err(error) => Err(error),
                };
            }
            if outcome.is_ok() || control.check().is_err() {
                break;
            }
            let error = outcome.as_ref().err().cloned().unwrap_or_default();
            if let Ok(db) = self.store.db.lock() {
                let _ = db.execute("UPDATE transfers SET attempts=?,error=?,updated_at=? WHERE id=? AND status='running'", params![attempt.saturating_add(1), redact_error(&error), now(), job.id]);
            }
            self.emit_changed(&job.id);
            let Some(delay) = retry_delay(attempt, &error, cfg!(target_os = "android")) else {
                break;
            };
            tokio::select! { _ = tokio::time::sleep(delay) => {}, _ = control.changed.notified() => {} }
            attempt = attempt.saturating_add(1);
        }
        #[cfg(target_os = "android")]
        let shared = shared_uri.is_some();
        #[cfg(not(target_os = "android"))]
        let shared = false;
        let (status, error) = if shared { ("completed", None) } else { match control.action.load(Ordering::Acquire) {
            1 => ("paused", None),
            2 => ("cancelled", None),
            _ => match outcome {
                Ok(()) => ("completed", None),
                Err(error) => ("failed", Some(redact_error(&error))),
            },
        }};
        let committed = if let Ok(db) = self.store.db.lock() {
            db.execute("UPDATE transfers SET status=?,error=?,updated_at=? WHERE id=? AND (status='running' OR (? AND status IN ('paused','cancelled')))", params![status, error, now(), job.id, shared]).unwrap_or(0) > 0
        } else {
            false
        };
        #[cfg(target_os = "android")]
        if let Some(uri) = shared_uri.as_deref().filter(|_| committed) {
            self.cleanup_shared_stage(&job.id, &job.local_path, uri);
        } else if status == "cancelled" && job.direction == TransferDirection::Download {
            if let Ok((Some(uri), false)) = self.shared_download_info(&job.id) {
                if let Err(error) = discard_shared_stage(&self.app, &job.local_path, &uri).await {
                    eprintln!("下载暂存清理失败：{error}");
                }
            }
        }
        if job.direction == TransferDirection::Download
            && (control.action.load(Ordering::Acquire) == 2 || (status == "completed" && committed))
        {
            let _ = fs::remove_file(part_path(&job.local_path, &job.id)).await;
        }
        if !job.repository && job.direction == TransferDirection::Upload && status == "completed" && committed {
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
                    source_id: None,
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
        {
            let session = self.client.read().await;
            if let Ok(db) = self.store.db.lock() {
                if let Ok(mut active) = self.active.lock() {
                    let stored_status: String = db
                        .query_row(
                            "SELECT status FROM transfers WHERE id=?",
                            [&job.id],
                            |row| row.get(0),
                        )
                        .unwrap_or_else(|_| status.into());
                    if matches!(stored_status.as_str(), "paused" | "failed")
                        && control.resume_after_stop.swap(false, Ordering::AcqRel)
                    {
                        if let Some((_, scope)) = session.as_ref() {
                            let _ = db.execute("UPDATE transfers SET status='queued',error=NULL,attempts=0,updated_at=? WHERE id=? AND account_scope=? AND status IN ('paused','failed')", params![now(), job.id, scope]);
                        }
                    }
                    if let Ok(mut activity) = self.activity.lock() {
                        activity.set_status(&job.id, &stored_status);
                    }
                    active.remove(&job.id);
                }
            }
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
        let previous: DownloadJournal = self
            .journal(&job.id)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if destination.exists() {
            let durable = self.saved_completed(&job.id)?;
            let metadata = fs::metadata(&destination)
                .await
                .map_err(|e| e.to_string())?;
            let published = same_file(&temporary, &destination);
            #[cfg(target_os = "android")]
            let published = published
                || (!temporary.exists()
                    && previous.publication.as_ref() == Some(&download_identity(&metadata)));
            if published
                && metadata.len() == durable
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
        // A crash/pause can happen after the final durable checkpoint but before
        // publishing. Requesting bytes=total- in that case only produces HTTP 416.
        if download_checkpoint_complete(offset, job.total_bytes) {
            control.check()?;
            file.sync_all().await.map_err(|e| e.to_string())?;
            drop(file);
            self.publish_download(job, &temporary, &destination).await?;
            self.checkpoint(&job.id, offset, offset)?;
            self.progress(job, offset, offset, 0.0, "completed");
            return Ok(());
        }
        let source_client;
        let download_client = if let Some(id) = &job.remote.source_id {
            let owner = client.account_config().await.id;
            source_client = crate::sessions::Sessions::load(&self.store)?
                .source(&owner, id, true)?.client(client).await?;
            &source_client
        } else {
            client
        };
        let (response, request_offset) = download_with_fresh_link(
            &self.http,
            || download_client.link(&job.remote),
            offset,
            previous.validator.as_deref(),
            control,
        )
        .await?;
        let status = response.status();
        if response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|encoding| encoding != "identity")
        {
            return Err("服务器返回了压缩编码，无法安全进行断点下载".into());
        }
        if !status.is_success() {
            return Err(format!("下载请求返回 HTTP {}", status.as_u16()));
        }
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok());
        let disposition = resume_disposition(
            status.as_u16(),
            request_offset,
            content_range,
            job.total_bytes,
        )?;
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
        if disposition == ResumeDisposition::Append && request_offset > 0 {
            if let (Some(old), Some(new)) = (&previous.validator, &validator) {
                if old != new {
                    return Err("云端文件版本已变化，请重新添加任务".into());
                }
            }
        }
        // Keep the durable partial file until the fresh response passes validation.
        if disposition == ResumeDisposition::Restart || request_offset == 0 {
            file.set_len(0).await.map_err(|e| e.to_string())?;
            offset = 0;
        }
        self.save_journal(
            &job.id,
            &DownloadJournal {
                validator,
                publication: None,
            },
        )?;
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
        self.publish_download(job, &temporary, &destination).await?;
        self.checkpoint(&job.id, completed, completed)?;
        self.progress(job, completed, completed, 0.0, "completed");
        Ok(())
    }

    async fn publish_download(
        &self,
        _job: &TransferJob,
        temporary: &Path,
        destination: &Path,
    ) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            // Persist the source inode before rename. If the process stops after
            // rename but before completion is committed, only this exact file
            // can be accepted on recovery, never an unrelated existing file.
            let mut journal: DownloadJournal = self
                .journal(&_job.id)?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            journal.publication = Some(download_identity(
                &fs::metadata(temporary).await.map_err(|e| e.to_string())?,
            ));
            self.save_journal(&_job.id, &journal)?;
        }
        publish_download(temporary, destination).await
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
        let mut renewed_part = false;
        'parts: while journal.finished_parts < part_count {
            let start_number = journal.finished_parts + 1;
            control.check()?;
            // A batch of 100 URLs can expire long before its tail is reached on
            // a slow TB transfer. Refresh the next part just before sending it.
            let parts = if start_number == 1 && first_part.is_some() {
                vec![first_part.take().expect("checked initial upload part")]
            } else {
                client
                    .refresh_upload_parts(&journal.plan, start_number, 1)
                    .await?
            };
            if parts.len() != 1 {
                return Err("云端未返回请求的上传分片地址，请重试".into());
            }
            for part in parts {
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
                let transfer_activity = self.activity.clone();
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
                            publish_transfer_progress(
                                &manager_app,
                                &transfer_activity,
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
                let result = tokio::select! {
                    result = client.upload_part(&part, body) => result,
                    _ = stalled => return Err("上传超过 120 秒没有进展，保留已确认分片并自动重试".into()),
                    _ = control.changed.notified() => { control.check()?; return Err("上传已停止".into()); }
                };
                if matches!(&result, Err(crate::cloud::UploadPartError::Rejected(reqwest::StatusCode::FORBIDDEN)))
                    && !renewed_part
                {
                    renewed_part = true;
                    continue 'parts;
                }
                result?;
                renewed_part = false;
                control.check()?;
                journal.finished_parts = part.part_number;
                completed += part.size;
                self.save_journal(&job.id, &journal)?;
                self.checkpoint(&job.id, completed, size)?;
                self.progress(job, completed, size, 0.0, "transferring");
            }
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

    pub(crate) async fn open_source(&self, path: &str) -> Result<fs::File, String> {
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
    pub(crate) fn journal(&self, id: &str) -> Result<Option<String>, String> {
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
    pub(crate) fn save_journal<T: Serialize>(&self, id: &str, journal: &T) -> Result<(), String> {
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
    pub(crate) fn checkpoint(&self, id: &str, completed: u64, total: u64) -> Result<(), String> {
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
    pub(crate) fn progress(&self, job: &TransferJob, completed: u64, total: u64, speed: f64, phase: &str) {
        publish_transfer_progress(
            &self.app,
            &self.activity,
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
    async fn run_repository(self:&Arc<Self>,job:&TransferJob,client:&CloudClient,control:&Arc<Control>)->Result<(),String>{
        let encoded:String=self.store.db.lock().map_err(|_|"传输数据库不可用")?.query_row("SELECT repository_state FROM transfers WHERE id=?",[&job.id],|row|row.get(0)).map_err(|_|"传输断点不可用")?;
        if encoded.len()>48<<20{return Err("传输断点过大".into())}
        let raw=zeroize::Zeroizing::new(self.store.decrypt(&STANDARD.decode(encoded).map_err(|_|"传输断点已损坏")?)?);
        let intent:SpecialTransfer=serde_json::from_slice(&raw).map_err(|_|"传输断点已损坏")?;
        match intent {
            SpecialTransfer::OpenListCrypt(crypt_intent) => {
                self.run_crypt_transfer(job, client, control, crypt_intent).await
            }
            SpecialTransfer::Repository(repo_intent) => {
                let repository=self.app.state::<Arc<crate::repositories::RepositoryManager>>().inner().clone();
                crate::repositories::queue::run(repository,self.app.clone(),client.account_config().await,repo_intent,job.clone(),control.clone(),self.clone()).await
            }
        }
    }
}

#[cfg(target_os = "android")]
async fn discard_shared_stage(app: &AppHandle, path: &str, uri: &str) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("Downloads");
    let identity = if Path::new(path).starts_with(root) {
        std::fs::metadata(path).ok().map(|meta| download_identity(&meta))
    } else {
        None
    };
    app.document_picker().discard_shared_download(uri).await?;
    if identity.is_some() && identity == std::fs::metadata(path).ok().map(|meta| download_identity(&meta)) {
        if let Err(error) = fs::remove_file(path).await {
            if error.kind() != std::io::ErrorKind::NotFound { return Err(error.to_string()); }
        }
    }
    Ok(())
}

// Upload body streams and regular checkpoints must update the same activity
// sample before notifying the UI, so the overview and tray see live bandwidth.
fn record_transfer_progress(activity: &Mutex<TransferActivity>, progress: &TransferProgress) {
    if let Ok(mut activity) = activity.lock() {
        activity.progress(
            &progress.id,
            progress.completed_bytes,
            progress.total_bytes,
            progress.speed,
            &progress.phase,
        );
    }
}

fn publish_transfer_progress(
    app: &AppHandle,
    activity: &Mutex<TransferActivity>,
    progress: TransferProgress,
) {
    record_transfer_progress(activity, &progress);
    let _ = app.emit("transfer-progress", progress);
}

fn retry_delay(attempt: u32, error: &str, android: bool) -> Option<Duration> {
    let lower = error.to_ascii_lowercase();
    let transient = [
        "connection",
        "network",
        "dns error",
        "error sending request",
        "timed out",
        "timeout",
        "http 429",
        "http 5",
        "网络",
        "超时",
        "没有进展",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if attempt >= 4 && !(android && transient) {
        return None;
    }
    Some(Duration::from_secs(
        (1u64 << attempt.min(5)).min(30) + rand::random::<u64>() % 2,
    ))
}

fn transfer_filter(filter: &str) -> Result<(String, &'static str), String> {
    let (base, errors) = filter
        .strip_suffix(":errors")
        .map_or((filter, false), |base| (base, true));
    let condition = match base {
        "upload_active" => "direction='upload' AND status NOT IN ('completed','cancelled')",
        "upload_completed" => "direction='upload' AND status='completed'",
        "download_active" => "direction='download' AND status NOT IN ('completed','cancelled')",
        "download_completed" => "direction='download' AND status='completed'",
        "active" => "status NOT IN ('completed','cancelled')",
        "completed" => "status='completed'",
        "all" => "1=1",
        _ => return Err("不支持的传输筛选条件".into()),
    };
    let predicate = format!(
        "WHERE {condition}{}",
        if errors { " AND status='failed'" } else { "" }
    );
    Ok((predicate, "CASE status WHEN 'running' THEN 0 WHEN 'failed' THEN 1 WHEN 'paused' THEN 2 ELSE 3 END,created_at DESC,rowid DESC"))
}

fn activity_job(job: &TransferJob) -> ActivityJob {
    ActivityJob {
        id: job.id.clone(),
        upload: job.direction == TransferDirection::Upload,
        total: job.total_bytes,
        completed: job.completed_bytes,
        status: job.status.clone(),
    }
}

fn resume_scope(
    db: &mut rusqlite::Connection,
    scope: &str,
    active: &HashSet<String>,
) -> Result<usize, String> {
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    let ids = {
        let mut query = transaction
            .prepare(
                "SELECT id FROM transfers WHERE account_scope=? AND status IN ('paused','failed')",
            )
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([scope], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut count = 0;
    for id in ids.into_iter().filter(|id| !active.contains(id)) {
        count += transaction.execute("UPDATE transfers SET status='queued',error=NULL,attempts=0,updated_at=? WHERE id=?", params![now(), id]).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(count)
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
        repository:row.get(12)?,
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
    #[cfg(target_os = "android")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let from = CString::new(temporary.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let to = CString::new(destination.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // Android SELinux rejects app hard links even in private storage.
        // renameat2 publishes atomically and RENAME_NOREPLACE prevents overwrites.
        // syscall also works on older Android versions without a bionic wrapper.
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(format!(
                "无法发布完整下载，断点已保留：{}",
                std::io::Error::last_os_error()
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::File::open(parent)
                .await
                .map_err(|e| e.to_string())?
                .sync_all()
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
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
                    let from: Vec<u16> =
                        temporary.as_os_str().encode_wide().chain(Some(0)).collect();
                    let to: Vec<u16> = destination
                        .as_os_str()
                        .encode_wide()
                        .chain(Some(0))
                        .collect();
                    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) }
                        == 0
                    {
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

fn download_checkpoint_complete(offset: u64, total: u64) -> bool {
    total > 0 && offset == total
}

async fn download_with_fresh_link<F, Link>(
    http: &reqwest::Client,
    mut resolve: F,
    offset: u64,
    validator: Option<&str>,
    control: &Control,
) -> Result<(reqwest::Response, u64), String>
where
    F: FnMut() -> Link,
    Link: std::future::Future<Output = Result<String, String>>,
{
    control.check()?;
    let link = resolve().await?;
    let result = download_response(http, &link, offset, validator, control).await?;
    if matches!(
        result.0.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        drop(result);
        control.check()?;
        let fresh = resolve().await?;
        return download_response(http, &fresh, offset, validator, control).await;
    }
    Ok(result)
}

async fn download_response(
    http: &reqwest::Client,
    link: &str,
    offset: u64,
    validator: Option<&str>,
    control: &Control,
) -> Result<(reqwest::Response, u64), String> {
    let mut request_offset = offset;
    loop {
        control.check()?;
        let mut request = http
            .get(link)
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if request_offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={request_offset}-"));
            if let Some(value) = validator {
                request = request.header(reqwest::header::IF_RANGE, value);
            }
        }
        let response = tokio::select! {
            result = request.send() => result.map_err(|e| e.without_url().to_string())?,
            _ = control.changed.notified() => {
                control.check()?;
                return Err("请求已中止".into());
            }
        };
        // Some download nodes reject a saved range (sometimes without a
        // Content-Range header). Retry once without Range/If-Range; the caller
        // validates the response and restarts rather than appending its bytes.
        if response.status().as_u16() == 416 && request_offset > 0 {
            request_offset = 0;
            continue;
        }
        return Ok((response, request_offset));
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
    #[test]
    fn bulk_resume_preserves_accounts_and_waits_for_stopping_workers() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE transfers(id TEXT,status TEXT,account_scope TEXT,error TEXT,attempts INTEGER,updated_at INTEGER);
          INSERT INTO transfers VALUES ('paused','paused','a','err',3,0),('failed','failed','a','err',3,0),('stopping','paused','a',NULL,0,0),('other','paused','b',NULL,0,0),('done','completed','a',NULL,0,0);").unwrap();
        assert_eq!(
            super::resume_scope(
                &mut db,
                "a",
                &std::collections::HashSet::from(["stopping".into()])
            )
            .unwrap(),
            2
        );
        let count: i64 = db.query_row("SELECT COUNT(*) FROM transfers WHERE status='queued' AND error IS NULL AND attempts=0", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 2);
        let paused: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM transfers WHERE status='paused'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(paused, 2);
    }
    #[test]
    fn android_network_retries_continue_but_permanent_errors_stop() {
        assert!(super::retry_delay(20, "connection reset", true).is_some());
        assert!(super::retry_delay(20, "HTTP 503", true).is_some());
        assert!(super::retry_delay(20, "connection reset", false).is_none());
        assert!(super::retry_delay(20, "本地文件已修改", true).is_none());
    }
    #[test]
    fn transfer_categories_filter_before_pagination() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE transfers(direction TEXT,status TEXT); INSERT INTO transfers VALUES ('upload','running'),('upload','failed'),('upload','completed'),('download','failed'),('download','completed'),('download','cancelled');").unwrap();
        for (filter, expected) in [
            ("upload_active", 2),
            ("upload_completed", 1),
            ("download_active:errors", 1),
            ("active", 3),
            ("completed", 2),
        ] {
            let (predicate, _) = super::transfer_filter(filter).unwrap();
            let count: i64 = db
                .query_row(
                    &format!("SELECT COUNT(*) FROM transfers {predicate}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, expected, "{filter}");
        }
    }

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
    fn streamed_upload_samples_reach_the_overview_and_tray_accounting() {
        let activity = Mutex::new(TransferActivity::default());
        activity.lock().unwrap().observe(vec![
            ActivityJob {
                id: "upload-a".into(),
                upload: true,
                total: 1000,
                completed: 0,
                status: "running".into(),
            },
            ActivityJob {
                id: "upload-b".into(),
                upload: true,
                total: 1000,
                completed: 0,
                status: "running".into(),
            },
            ActivityJob {
                id: "download".into(),
                upload: false,
                total: 1000,
                completed: 0,
                status: "running".into(),
            },
        ]);
        for (id, done, speed) in [
            ("upload-a", 300, 252.0),
            ("upload-b", 200, 107.0),
            ("download", 400, 176.0),
        ] {
            record_transfer_progress(
                &activity,
                &TransferProgress {
                    id: id.into(),
                    completed_bytes: done,
                    total_bytes: 1000,
                    speed,
                    phase: "transferring".into(),
                },
            );
        }
        // Periodic database observations must retain in-flight stream samples.
        activity.lock().unwrap().observe(vec![ActivityJob {
            id: "upload-a".into(),
            upload: true,
            total: 1000,
            completed: 0,
            status: "running".into(),
        }]);
        let totals = activity.lock().unwrap().overview();
        assert_eq!(totals.upload_speed, 359.0);
        assert_eq!(totals.download_speed, 176.0);
        assert_eq!(totals.completed_bytes, 900);
        activity.lock().unwrap().set_status("upload-a", "paused");
        assert_eq!(activity.lock().unwrap().overview().upload_speed, 107.0);
    }

    #[test]
    fn download_journal_preserves_publication_identity_and_accepts_legacy_checkpoints() {
        let old: DownloadJournal = serde_json::from_str(r#"{"validator":"v1"}"#).unwrap();
        assert!(old.publication.is_none());
        let journal = DownloadJournal {
            validator: old.validator,
            publication: Some(DownloadPublication {
                device: 7,
                inode: 42,
                bytes: 106561664,
            }),
        };
        let recovered: DownloadJournal =
            serde_json::from_str(&serde_json::to_string(&journal).unwrap()).unwrap();
        assert_eq!(recovered.publication, journal.publication);
        assert_ne!(
            recovered.publication,
            Some(DownloadPublication {
                device: 7,
                inode: 43,
                bytes: 106561664
            })
        );
    }

    #[test]
    fn fully_synced_download_can_publish_without_requesting_past_eof() {
        assert!(download_checkpoint_complete(106561664, 106561664));
        assert!(!download_checkpoint_complete(106561663, 106561664));
        assert!(!download_checkpoint_complete(106561665, 106561664));
        assert!(!download_checkpoint_complete(0, 0));
    }

    fn serve_download_responses(
        replies: &'static [&str],
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    assert_eq!(stream.read(&mut byte).unwrap(), 1);
                    bytes.push(byte[0]);
                }
                requests.push(String::from_utf8(bytes).unwrap().to_lowercase());
                stream.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        (url, server)
    }

    #[tokio::test]
    async fn rejected_resume_retries_once_without_range_or_validator() {
        let (url, server) = serve_download_responses(&[
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata",
        ]);
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let (response, offset) = download_response(&http, &url, 3, Some("\"v1\""), &Control::new())
            .await
            .unwrap();
        assert_eq!(offset, 0);
        assert_eq!(response.status(), 200);
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"data");
        let requests = server.join().unwrap();
        assert!(requests[0].contains("range: bytes=3-\r\n"));
        assert!(requests[0].contains("if-range: \"v1\"\r\n"));
        assert!(!requests[1].contains("range:"));
        assert!(requests[1].contains("accept-encoding: identity"));
    }

    #[tokio::test]
    async fn expired_download_renews_link_and_keeps_range_and_validator() {
        let (url, server) = serve_download_responses(&[
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-4/5\r\nContent-Length: 2\r\nConnection: close\r\n\r\nta",
        ]);
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut links = ["expired", "renewed"].into_iter();
        let (response, offset) = download_with_fresh_link(
            &http,
            || std::future::ready(Ok(format!("{url}?grant={}", links.next().unwrap()))),
            3,
            Some("\"v1\""),
            &Control::new(),
        )
        .await
        .unwrap();
        assert_eq!(offset, 3);
        assert_eq!(response.status(), 206);
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"ta");
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("get /file?grant=expired "));
        assert!(requests[1].starts_with("get /file?grant=renewed "));
        for request in requests {
            assert!(request.contains("range: bytes=3-\r\n"));
            assert!(request.contains("if-range: \"v1\"\r\n"));
        }
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
        #[cfg(not(target_os = "android"))]
        assert!(same_file(&partial, &destination));
        #[cfg(target_os = "android")]
        {
            assert!(!partial.exists());
            assert_eq!(
                fs::read(&destination).await.unwrap(),
                b"completed-file-bytes"
            );
            fs::write(&partial, b"completed-file-bytes").await.unwrap();
        }
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
