//! Native AxuanStor sessions. Unlocks and provider secrets never enter status
//! DTOs. Payload sources come from native selection or the file-drop flow.
use crate::{cloud::{AccountConfig,CloudFile}, system_secrets, AppState};
use axuanstor_sdk::{
    self as sdk,
    cmcloud::{MobileCloud, WriteOptions},
    descriptor::Descriptor,
    repository::{
        digest, logical_path, DirectoryItem, Repository, Snapshot, Store as ObjectStore,
        UploadPlan, UploadSource,
    },
};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap,HashSet},
    io::{Read,Write},
    path::PathBuf,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};
#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::{AppHandle, State, Emitter};
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "android")]
use tauri_plugin_document_picker::DocumentPickerExt;
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
use zeroize::{Zeroize, Zeroizing};
use crate::repository_index::DirectoryIndex;
#[path = "repository_cache.rs"]
pub mod cache;
#[path = "repository_queue.rs"]
pub(crate) mod queue;
#[path = "repository_preview.rs"]
pub(crate) mod preview;
#[path = "repository_media.rs"]
mod media;
#[path="repository_sources.rs"]
mod sources;
#[path="repository_bridge.rs"]
pub mod bridge;

pub struct RepositoryManager {
	payload_slot:tokio::sync::Semaphore,
	previews:Mutex<HashMap<String,preview::Preview>>,
    playbacks: Mutex<HashMap<String, media::Playback>>,
	cache_catalog: Mutex<()>,
    active_transfers:Mutex<HashMap<(String,String),usize>>,
    directory: PathBuf,
    cache: PathBuf,
    factories: Mutex<Vec<(String, Arc<MobileCloud>)>>,
    hot_caches: Mutex<HashMap<String, Arc<sdk::metadata_cache::HotMetadata>>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    keys: Mutex<HashMap<String, ImportedKey>>,
    downloads: Mutex<HashMap<String, PathBuf>>,
}
struct ImportedKey {
    scope: String,
    key: Zeroizing<[u8; 32]>,
}
pub(crate) struct Session {
    scope: String,
    folder: String,
    backend: Arc<MobileCloud>,
    maintenance_backend: Arc<MobileCloud>,
    maintenance_store: Arc<dyn ObjectStore>,
    observation: PathBuf,
    index_directory: PathBuf,
    state: Mutex<SessionState>,
    compact_running: AtomicBool,
    compact_after: Mutex<u64>,
}
pub(crate) struct SessionState {
    inventory:Option<(std::time::Instant,Vec<sdk::repository::ObjectRef>)>,
    complete_snapshot:Option<(String,Snapshot)>,
    repo: Repository,
    pending: Option<Pending>,
    view: Option<(String, Snapshot)>,
    sync_revision: String,
}
struct Pending {
    id: String,
    basis: Snapshot,
    sources: Vec<UploadSource>,
    plan: UploadPlan,
    local_paths:Vec<String>,
    queued:bool,
    directories:Vec<String>,
    source_root:String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    folder_id: String,
    repository_id: Option<String>,
    kind: Option<String>,
    state: String,
    saved: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    handle: String,
    repository_id: String,
    folder_id: String,
    revision: String,
    encrypted: bool,
    directory: String,
    items: Vec<Item>,
    conflicts: Vec<String>,
    historical: bool,
    saved: bool,
    credential_revision: String,
    unlock_methods: Vec<String>,
    can_change_unlock: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    #[serde(flatten)]
    item: DirectoryItem,
    protected: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    id: String,
    #[serde(flatten)]
    plan: UploadPlan,
    directories:usize,
    skipped:usize,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Unlock {
    #[serde(default)]
    password: String,
    #[serde(default)]
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_file: Option<String>,
}
impl Drop for Unlock {
    fn drop(&mut self) {
        self.password.zeroize();
        self.key.zeroize()
    }
}

fn error(e: sdk::Error) -> String {
    match e {
        sdk::Error::Locked => "需要正确的密码、密钥或已登记设备的目录授权".into(),
        sdk::Error::Conflict => "目录或文件发生了并发变化，请刷新后重新确认".into(),
        sdk::Error::NotFound => "未找到该对象库文件".into(),
        sdk::Error::Recovery => {
            "上一次云盘操作尚未确认，原上传会话和进度已保留，需核查后恢复".into()
        }
        sdk::Error::Budget => {
            "本机加密暂存额度或可用空间不足，请保留当前缓存并释放空间后重试".into()
        }
        sdk::Error::Incomplete => "对象库暂时无法完整读取，请稍后刷新；原进度已保留".into(),
        sdk::Error::Integrity | sdk::Error::Invalid => {
            "对象库内容或文件校验未通过，操作已停止".into()
        }
        sdk::Error::Unsupported => "当前对象库能力或云空间尚未支持".into(),
        sdk::Error::Io(_) => "本机或云端读写失败，原文件与检查点已保留".into(),
    }
}
pub(crate) fn scope(c: &AccountConfig) -> String {
    digest(
        format!(
            "{}\0{}\0{}\0{}",
            c.id, c.space, c.cloud_id, c.user_domain_id
        )
        .as_bytes(),
    )
}
fn secret_id(scope: &str, repo: &str) -> String {
    format!("repo:{}", digest(format!("{scope}\0{repo}").as_bytes()))
}
fn key_bytes(s: &str) -> Result<Option<Zeroizing<[u8; 32]>>, String> {
    if s.is_empty() {
        return Ok(None);
    };
    let bytes = if s.starts_with("AXUANSTOR-KEY-v1:") {
        STANDARD_NO_PAD
            .decode(s.trim().strip_prefix("AXUANSTOR-KEY-v1:").unwrap_or(""))
            .map_err(|_| "密钥格式无效")?
    } else {
        hex::decode(s.trim())
            .or_else(|_| STANDARD_NO_PAD.decode(s.trim()))
            .map_err(|_| "请输入 32 字节密钥，或导入 AxuanStor 密钥文件")?
    };
    let b = Zeroizing::new(bytes);
    let key: [u8; 32] = b.as_slice().try_into().map_err(|_| "密钥必须是 32 字节")?;
    Ok(Some(Zeroizing::new(key)))
}
fn marker(backend: &MobileCloud) -> sdk::Result<Descriptor> {
    let mut found = None;
    for r in backend.list_named(sdk::repository::MARKER)? {
        if r.key != sdk::repository::MARKER {
            continue;
        };
        if found
            .as_ref()
            .is_some_and(|old: &sdk::repository::ObjectRef| old.id != r.id)
        {
            return Err(sdk::Error::Conflict);
        };
        found = Some(r)
    }
    let r = found.ok_or(sdk::Error::NotFound)?;
    if r.is_dir || r.size > 65536 {
        return Err(sdk::Error::Invalid);
    };
    let mut b = vec![];
    backend.open(&r)?.take(65537).read_to_end(&mut b)?;
    Descriptor::parse(&b)
}
impl RepositoryManager {
    pub fn new(directory: PathBuf, cache: PathBuf) -> Result<Self, String> {
        if !directory.is_absolute() || !cache.is_absolute() {
            return Err("对象库本机目录无效".into());
        };
        std::fs::create_dir_all(&directory).map_err(|_| "对象库本机状态目录不可用")?;
        Ok(Self {
            payload_slot:tokio::sync::Semaphore::new(1),
            previews:Mutex::new(HashMap::new()),
            playbacks: Mutex::new(HashMap::new()),
            active_transfers:Mutex::new(HashMap::new()),
			cache_catalog: Mutex::new(()),
            directory,
            cache,
            factories: Mutex::new(vec![]),
            hot_caches: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            keys: Mutex::new(HashMap::new()),
            downloads: Mutex::new(HashMap::new()),
        })
    }
    fn insert_session(&self, handle: String, session: Arc<Session>) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|_| "对象库会话不可用")?;
        if sessions.len() >= 8 {
            return Err("打开的对象库过多，请先退出不再使用的对象库".into());
        };
        sessions.insert(handle, session);
        Ok(())
    }
    fn close(&self, handle: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.remove(handle) {
                if let Ok(mut playbacks) = self.playbacks.lock() {
                    playbacks.retain(|_, playback| playback.handle != handle);
                }
                session.backend.cancel_flag().store(true, Ordering::Release);
                session.maintenance_backend.cancel_flag().store(true, Ordering::Release)
            }
        }
    }
    fn materialize(&self, owner: &str, u: &mut Unlock) -> Result<(), String> {
        if let Some(id) = u.key_file.take() {
            let keys = self.keys.lock().map_err(|_| "密钥选择已中断")?;
            let k = keys.get(&id).ok_or("密钥选择已过期，请重新导入")?;
            if k.scope != owner {
                return Err("请在选择密钥的原账号中继续操作".into());
            };
            u.key = hex::encode(&*k.key)
        };
        Ok(())
    }
    fn create(
        &self,
        c: &AccountConfig,
        folder: &str,
        encrypted: bool,
        u: &Unlock,
    ) -> Result<(String, String), String> {
        let backend = self.backend(c, folder, true)?;
        let cached = self.cached_backend(c, folder, backend.clone())?;
        let (maintenance_backend,maintenance_store)=self.maintenance_backend(c,folder,&backend)?;
        let key = key_bytes(&u.key)?;
        let repo = Repository::create(
            cached,
            encrypted,
            (!u.password.is_empty()).then_some(u.password.as_str()),
            key.as_ref().map(|x| &**x),
        )
        .map_err(error)?;
        let owner = scope(c);
        let sid = secret_id(&owner, &repo.descriptor.id);
        let observation = self
            .directory
            .join(format!("{}-{}.observed.json", owner, repo.descriptor.id));
        let handle = uuid::Uuid::new_v4().to_string();
        self.insert_session(
            handle.clone(),
            Arc::new(Session {
                compact_running: AtomicBool::new(false),
                compact_after: Mutex::new(0),
                scope: owner,
                folder: folder.into(),
                backend,
                maintenance_backend,
                maintenance_store,
                observation,
                index_directory:self.directory.join("views").join(scope(c)),
                state: Mutex::new(SessionState {
                    inventory:None,
                    complete_snapshot:None,
                    repo,
                    pending: None,
                    sync_revision: String::new(),
                    view: None,
                }),
            }),
        )?;
        Ok((handle, sid))
    }
    fn backend(
        &self,
        c: &AccountConfig,
        folder: &str,
        write: bool,
    ) -> Result<Arc<MobileCloud>, String> {
        if c.space != "personal_new" {
            return Err("AxuanStor 当前使用个人云中的专用文件夹".into());
        };
        let owner = scope(c);
        let id = digest(format!("{owner}\0{}", c.authorization).as_bytes());
        let mut factories = self.factories.lock().map_err(|_| "对象库会话不可用")?;
        let existing = factories
            .iter()
            .find(|(k, _)| k == &id)
            .map(|(_, v)| v.clone());
        let factory = match existing {
            Some(v) => v,
            None => {
                let v = Arc::new(
                    MobileCloud::new(c.authorization.clone(), folder.into(), None)
                        .map_err(error)?,
                );
                if factories.len() >= 4 {
                    factories.remove(0);
                };
                factories.push((id, v.clone()));
                v
            }
        };
        let options = write.then(|| WriteOptions {
            journal_directory: self
                .directory
                .join("journals")
                .join(&owner)
                .join(digest(folder.as_bytes())),
            upload_bytes_per_second: 0,
        });
        Ok(Arc::new(
            factory.for_folder(folder.into(), options).map_err(error)?,
        ))
    }
    fn session(&self, handle: &str, scope: &str) -> Result<Arc<Session>, String> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| "对象库会话不可用")?
            .get(handle)
            .cloned()
            .ok_or("对象库已锁定或会话已结束")?;
        if session.scope != scope {
            return Err("请在打开对象库的原账号和云空间中继续操作".into());
        };
        Ok(session)
    }
    fn cached_backend(&self, c: &AccountConfig, folder: &str, backend: Arc<MobileCloud>) -> Result<Arc<dyn ObjectStore>, String> {
        let owner = scope(c);
        let folder_hash = digest(folder.as_bytes());
        let hot_key = format!("{owner}\0{folder_hash}");
        #[cfg(target_os = "android")]
        let (disk_cap, min_free, hot_cap) = (128u64 << 20, 512u64 << 20, 16usize << 20);
        #[cfg(not(target_os = "android"))]
        let (disk_cap, min_free, hot_cap) = (256u64 << 20, 2u64 << 30, 64usize << 20);
        let hot = {
            let mut guard = self.hot_caches.lock().map_err(|_| "对象库缓存不可用")?;
            if guard.len() >= 8 && !guard.contains_key(&hot_key) {
                guard.clear();
            }
            guard
                .entry(hot_key)
                .or_insert_with(|| Arc::new(sdk::metadata_cache::HotMetadata::new(hot_cap)))
                .clone()
        };
        Ok(Arc::new(
            sdk::metadata_cache::MetadataCache::new(
                backend,
                self.cache.join("repository-metadata").join(owner).join(folder_hash),
                disk_cap,
                min_free,
            )
            .map_err(error)?
            .with_hot(hot),
        ))
    }
    fn maintenance_backend(&self,c:&AccountConfig,folder:&str,backend:&MobileCloud)->Result<(Arc<MobileCloud>,Arc<dyn ObjectStore>),String>{
        let options=WriteOptions{journal_directory:self.directory.join("journals").join(scope(c)).join(digest(folder.as_bytes())),upload_bytes_per_second:1<<20};
        let limited=Arc::new(backend.for_folder(folder.into(),Some(options)).map_err(error)?.with_download_budget(sdk::rate::Budget::new(1<<20).map_err(error)?));
        let store=self.cached_backend(c,folder,limited.clone())?;Ok((limited,store))
    }
    fn probe(&self, c: &AccountConfig, folder: &str) -> Result<Probe, String> {
        let mut out = Probe {
            folder_id: folder.into(),
            repository_id: None,
            kind: None,
            state: "ordinary".into(),
            saved: false,
        };
        let backend = self.backend(c, folder, false)?;
        match marker(&backend) {
            Ok(d) => {
                out.repository_id = Some(d.id);
                out.kind = Some(d.kind);
                out.state = "repository".into()
            }
            Err(sdk::Error::NotFound) => {}
            Err(_) => out.state = "unavailable".into(),
        };
        Ok(out)
    }
    fn open(
        &self,
        c: &AccountConfig,
        folder: &str,
        expected: &str,
        u: &Unlock,
    ) -> Result<(String, String), String> {
        if expected.len()!=32||!expected.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err("对象库身份无效".into())}
        let owner=scope(c);
        let checkpoint=self.directory.join(format!("{owner}-{expected}.unlock-change.json"));
        let observation=self.directory.join(format!("{owner}-{expected}.observed.json"));
        let backend = self.backend(c, folder, true)?;
        let cached = self.cached_backend(c, folder, backend.clone())?;
        let (maintenance_backend,maintenance_store)=self.maintenance_backend(c,folder,&backend)?;
        let key = key_bytes(&u.key)?;
        let repo = Repository::open_with_local_state(
            cached,
            (!u.password.is_empty()).then_some(u.password.as_str()),
            key.as_ref().map(|x| &**x),
            Some(&checkpoint),
            Some(&observation),
        )
        .map_err(error)?;
        if repo.descriptor.id != expected {
            return Err("对象库身份已改变，请重新检查该文件夹".into());
        };
        let handle = uuid::Uuid::new_v4().to_string();
        let sid = secret_id(&owner, &repo.descriptor.id);
        self.insert_session(
            handle.clone(),
            Arc::new(Session {
                compact_running: AtomicBool::new(false),
                compact_after: Mutex::new(0),
                scope: owner,
                folder: folder.into(),
                backend,
                maintenance_backend,
                maintenance_store,
                observation,
                index_directory:self.directory.join("views").join(scope(c)),
                state: Mutex::new(SessionState {
                    inventory:None,
                    complete_snapshot:None,
                    repo,
                    pending: None,
                    sync_revision: String::new(),
                    view: None,
                }),
            }),
        )?;
        Ok((handle, sid))
    }
    fn view(
        &self,
        handle: &str,
        owner: &str,
        directory: &str,
        saved: bool,
        refresh: bool,
    ) -> Result<View, String> {
        let session = self.session(handle, owner)?;
        let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
        if refresh {
            state.inventory = None;
            state.complete_snapshot = None;
        }
        let (v,listed)=directory_state(&session,&mut state,directory,false)?;
        let revision=state.sync_revision.clone();
        let policy=state.repo.strong_policy();
        let items = listed
            .into_iter()
            .map(|mut item| {
                item.modified_at = item.file_modified_at();
                Item {
                    protected: policy.as_ref().is_some_and(|p| p.protects(&item.path)),
                    item,
                }
            })
            .collect();
        let conflicts=v.conflicts.clone();
        state.view=Some((revision.clone(),v));
        let result=View {
            handle: handle.into(),
            repository_id: state.repo.descriptor.id.clone(),
            folder_id: session.folder.clone(),
            revision,
            encrypted: state.repo.descriptor.kind == "encrypted",
            directory: directory.into(),
            items,
            conflicts,
            historical: policy.as_ref().is_some_and(|p| p.protects(directory)),
            saved,
            credential_revision:state.repo.credential_revision().map_err(error)?,
            unlock_methods:state.repo.credential_methods(),
            can_change_unlock:state.repo.descriptor.credentials.is_some(),
        };
        drop(state);
        schedule_compact(session);
        Ok(result)
    }
}

#[derive(Default,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactStatus{phase:String,next_after:u64,failures:u32,metadata_candidates:usize,metadata_trashed:usize,semantic:String,#[serde(default)]source:String}
fn unix_now()->u64{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()}
/// Background work owns a separate SDK handle. User navigation never waits on
/// its repository mutex; immutable publications remain mergeable with writers.
fn schedule_compact(session:Arc<Session>){
    if cfg!(test){return}
    let now=unix_now();
    if session.compact_after.lock().map(|v|*v>now).unwrap_or(true)||session.compact_running.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return}
    std::thread::spawn(move||{
        let result=(||->sdk::Result<()>{
            let (fork,known_paths)={
                let state=session.state.lock().map_err(|_|sdk::Error::Locked)?;
                let inv_len=state.inventory.as_ref().map(|(_,r)|r.len()).unwrap_or(0);
                let kp=state.view.as_ref().map(|v|v.1.versions.len()).unwrap_or(0).max(state.repo.checkpoint_path_count() as usize);
                if inv_len > 2048 || kp > 500_000 {
                    if let Ok(mut after)=session.compact_after.lock(){*after=unix_now()+3600;}
                    return Ok(());
                }
                (state.repo.fork_for_maintenance()?,kp)
            };
            let mut repo=fork.fork_for_maintenance_using(session.maintenance_store.clone())?;
            sdk::local_state::directory(&session.index_directory)?;
            let base=session.index_directory.join(&repo.descriptor.id);
            let plan_path=base.with_extension("compact-plan.json");let status_path=base.with_extension("maintenance.json");
            let _single=sdk::local_state::lock(&base.with_extension("compact.lock"))?;
            let mut status=if status_path.try_exists()?{serde_json::from_slice::<CompactStatus>(&sdk::local_state::read(&status_path,8192)?).map_err(|_|sdk::Error::Integrity)?}else{CompactStatus::default()};
            if status.next_after>unix_now(){return Ok(())}
            let work=(||->sdk::Result<()>{
                let plan=if plan_path.try_exists()?{
                    serde_json::from_slice::<sdk::repository::CompactPlan>(&sdk::local_state::read(&plan_path,256<<20)?).map_err(|_|sdk::Error::Integrity)?
                }else{
                    if known_paths > 500_000 {
                        status.phase="large-repo-deferred".into();
                        status.next_after=unix_now()+3600;
                        return Ok(());
                    }
                    if !repo.compact_needed(known_paths)?{status.phase="idle".into();status.next_after=unix_now()+300;return Ok(())}
                    let known_paths = known_paths.max(repo.max_checkpoint_paths()? as usize);
                    if known_paths > 500_000 {
                        status.phase="large-repo-deferred".into();
                        status.next_after=unix_now()+3600;
                        return Ok(());
                    }
                    let inventory=repo.commit_inventory()?;
                    if inventory.len() > 2048 {
                        status.phase="large-repo-deferred".into();
                        status.next_after=unix_now()+3600;
                        return Ok(());
                    }
                    let stamp=digest(&serde_json::to_vec(&inventory).map_err(|_|sdk::Error::Invalid)?);let observed=repo.observed_state()?;
                    let mut index=DirectoryIndex::open(&session.index_directory,&repo).ok();
                    let mut cached=index.as_ref().filter(|index|index.matches(&stamp,&observed).unwrap_or(false)).and_then(|index|index.verified_snapshot(&repo).ok());
                    status.source=if cached.is_some(){"authenticated-complete-local-cache"}else{"verified-metadata-refresh"}.into();
                    if cached.is_none(){
                        if let Some(index)=index.as_mut(){let view=repo.refresh_inventory(&inventory)?;let observed=repo.observed_state()?;if index.replace(&repo,&view,&stamp,&observed).is_ok(){cached=index.verified_snapshot(&repo).ok();}}
                    }
                    let writer=digest(session.scope.as_bytes())[..32].to_string();
                    let plan=match cached{Some(snapshot)=>repo.prepare_compact_cached(&writer,snapshot,&inventory)?,None=>repo.prepare_compact(&writer)?};
                    let bytes=serde_json::to_vec(&plan).map_err(|_|sdk::Error::Invalid)?;
                    if bytes.len()>256<<20{return Err(sdk::Error::Budget)};
                    sdk::local_state::write(&plan_path,&bytes)?;plan
                };
                let receipt=repo.publish_compact(&plan,true)?;
                repo.save_observed_file(&session.observation)?;
                status=CompactStatus{phase:"complete".into(),next_after:unix_now()+300,failures:0,metadata_candidates:receipt.metadata_candidates,metadata_trashed:receipt.metadata_trashed,semantic:receipt.semantic,source:status.source.clone()};
                // It is a completed maintenance intent, not an SDK upload/session.
                std::fs::remove_file(&plan_path)?;Ok(())
            })();
            if work.is_err(){status.phase="deferred".into();status.failures=status.failures.saturating_add(1);status.next_after=unix_now()+30u64.saturating_mul(1u64<<status.failures.min(7)).min(3600);}
            sdk::local_state::write(&status_path,&serde_json::to_vec(&status).map_err(|_|sdk::Error::Invalid)?)?;
            if let Ok(mut after)=session.compact_after.lock(){*after=status.next_after;}
            work
        })();
        if result.is_err(){if let Ok(mut after)=session.compact_after.lock(){*after=unix_now()+600;}}
        session.compact_running.store(false,Ordering::Release);
    });
}

/// The SQL index is derived state. Unavailable/corrupt cached rows can never
/// make otherwise verified cloud state unreadable or replace durable observations.
fn directory_state(session:&Session,state:&mut SessionState,directory:&str,full:bool)->Result<(Snapshot,Vec<DirectoryItem>),String>{
    if session.observation.try_exists().map_err(|_|"对象库历史状态不可用")?{
        state.repo.restore_observed_file(&session.observation).map_err(error)?;
    }
    let mut index=DirectoryIndex::open(&session.index_directory,&state.repo).ok();
    let cached_inventory=if !full{
        state.inventory.as_ref().filter(|(at,refs)|{
            let ttl = if refs.len() > 1024 { 180 } else { 15 };
            at.elapsed()<std::time::Duration::from_secs(ttl)
        }).map(|(_,refs)|refs.clone())
    }else{None};
    if !full
        && cached_inventory.is_none()
        && state.complete_snapshot.is_none()
        && index.as_ref().is_none_or(|idx| !idx.has_directory(&state.repo, directory))
    {
        let (inventory, view) = state.repo.browse_directory_fresh(directory).map_err(error)?;
        state.inventory = Some((std::time::Instant::now(), inventory.clone()));
        let stamp = digest(&serde_json::to_vec(&inventory).map_err(|_| "对象库版本编码失败")?);
        state.repo.save_observed_file(&session.observation).map_err(error)?;
        let observations = state.repo.observed_state().map_err(error)?;
        if !view.used_compact {
            state.complete_snapshot = Some((stamp.clone(), view.snapshot.clone()));
            if let Some(index) = index.as_mut() {
                if view.snapshot.versions.len() <= 250_000 {
                    let _ = index.replace(&state.repo, &view.snapshot, &stamp, &observations);
                } else {
                    let _ = index.store_directory(&state.repo, &view, directory, &stamp, &observations);
                }
            }
        } else if let Some(index) = index.as_mut() {
            let _ = index.store_directory(&state.repo, &view, directory, &stamp, &observations);
        }
        state.sync_revision = view.semantic;
        return Ok((view.snapshot, view.items));
    }
    let inventory=match cached_inventory{Some(refs)=>refs,None=>{let refs=state.repo.commit_inventory().map_err(error)?;state.inventory=Some((std::time::Instant::now(),refs.clone()));refs}};
    let stamp=digest(&serde_json::to_vec(&inventory).map_err(|_|"对象库版本编码失败")?);
    if let Some((cached_stamp, cached_snap)) = &state.complete_snapshot {
        if cached_stamp == &stamp {
            state.sync_revision = cached_snap.semantic_revision().map_err(error)?;
            let items = cached_snap.list_directory(directory).map_err(error)?;
            return Ok((cached_snap.clone(), items));
        }
    }
    let observations=state.repo.observed_state().map_err(error)?;
    if let Some(index)=index.as_ref(){
        if index.matches(&stamp,&observations).unwrap_or(false){
            if full{
                if let Ok(snapshot)=index.snapshot(&state.repo){state.sync_revision=snapshot.semantic_revision().map_err(error)?;let items=snapshot.list_directory(directory).map_err(error)?;return Ok((snapshot,items))}
            }else if let Ok(Some(value))=index.read(&state.repo,directory){state.sync_revision=index.revision()?;return Ok(value)}
        }
    }
    // Keep the pre-scan references: a later eventually-consistent listing may
    // omit one, and a commit arriving during replay must trigger the next scan.
    if !full {
        let view=state.repo.browse_directory_inventory(directory,&inventory).map_err(error)?;
        state.repo.save_observed_file(&session.observation).map_err(error)?;
        let observations=state.repo.observed_state().map_err(error)?;
        if !view.used_compact {
            state.complete_snapshot = Some((stamp.clone(), view.snapshot.clone()));
            if let Some(index) = index.as_mut() {
                if view.snapshot.versions.len() <= 250_000 {
                    let _ = index.replace(&state.repo, &view.snapshot, &stamp, &observations);
                } else {
                    let _ = index.store_directory(&state.repo, &view, directory, &stamp, &observations);
                }
            }
        } else if let Some(index)=index.as_mut(){
            let _=index.store_directory(&state.repo,&view,directory,&stamp,&observations);
        }
        state.sync_revision=view.semantic;
        return Ok((view.snapshot,view.items));
    }
    let snapshot=state.repo.refresh_inventory(&inventory).map_err(error)?;
    state.sync_revision=snapshot.semantic_revision().map_err(error)?;
    state.complete_snapshot = Some((stamp.clone(), snapshot.clone()));
    state.repo.save_observed_file(&session.observation).map_err(error)?;
    let observations=state.repo.observed_state().map_err(error)?;
    if let Some(index)=index.as_mut(){
        if snapshot.versions.len() <= 250_000 {
            let _ = index.replace(&state.repo,&snapshot,&stamp,&observations);
        }
    }
    let items=snapshot.list_directory(directory).map_err(error)?;
    Ok((snapshot,items))
}

pub(crate) fn viewed_file_basis(
    session: &Session,
    state: &mut SessionState,
    revision: &str,
    path: &str,
) -> Result<Snapshot, String> {
    if let Some((shown, basis)) = &state.view {
        if shown == revision && basis.resolve(path).is_ok() {
            return Ok(basis.clone());
        }
    }
    let parent = path.rsplit_once('/').map(|pair| pair.0).unwrap_or("");
    let (view, _) = directory_state(session, state, parent, false)?;
    if state.sync_revision != revision {
        return Err("目录视图已更新，请重新选择文件".into());
    }
    Ok(view)
}

fn scoped_write_state(
    session: &Session,
    state: &mut SessionState,
    witness: &[String],
    subtrees: &[String],
) -> Result<(Snapshot, bool), String> {
    if session.observation.try_exists().map_err(|_| "对象库历史状态不可用")? {
        state.repo.restore_observed_file(&session.observation).map_err(error)?;
    }
    let inventory = state.repo.commit_inventory().map_err(error)?;
    state.inventory = Some((std::time::Instant::now(), inventory.clone()));
    let stamp = digest(&serde_json::to_vec(&inventory).map_err(|_| "对象库版本编码失败")?);
    let mut index = DirectoryIndex::open(&session.index_directory, &state.repo).ok();
    let observations = state.repo.observed_state().map_err(error)?;
    if let Some(index) = index.as_ref() {
        if index.matches(&stamp, &observations).unwrap_or(false) {
            if let Ok(snapshot) = index.snapshot(&state.repo) {
                state.sync_revision = snapshot.semantic_revision().map_err(error)?;
                return Ok((snapshot, false));
            }
        }
    }
    let (snapshot, semantic, used_compact) = state
        .repo
        .browse_paths_scoped_inventory(witness, subtrees, &inventory)
        .map_err(error)?;
    state.sync_revision = semantic;
    state.repo.save_observed_file(&session.observation).map_err(error)?;
    if !used_compact {
        let observations = state.repo.observed_state().map_err(error)?;
        if let Some(index) = index.as_mut() {
            let _ = index.replace(&state.repo, &snapshot, &stamp, &observations);
        }
    }
    Ok((snapshot, used_compact))
}

struct NewSession {
    manager: Arc<RepositoryManager>,
    handle: String,
    accepted: bool,
}
impl Drop for NewSession {
    fn drop(&mut self) {
        if !self.accepted {
            self.manager.close(&self.handle)
        }
    }
}
async fn finish_open(
    app: &AppHandle,
    state: &AppState,
    manager: Arc<RepositoryManager>,
    handle: String,
    sid: String,
    owner: String,
    sealed: Option<Zeroizing<Vec<u8>>>,
) -> Result<View, String> {
    let mut guard = NewSession {
        manager: manager.clone(),
        handle: handle.clone(),
        accepted: false,
    };
    if let Some(bytes) = sealed {
        system_secrets::save(&state.store, Some(app), &sid, &bytes).await?;
    }
    let saved = system_secrets::exists(&state.store, &sid)?;
    let view = tokio::task::spawn_blocking(move || manager.view(&handle, &owner, "", saved, false))
        .await
        .map_err(|_| "对象库读取已中断")??;
    guard.accepted = true;
    Ok(view)
}
struct CancelReader<R> {
    inner: R,
    cancel: Arc<AtomicBool>,
}
impl<R: Read> Read for CancelReader<R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "cancelled",
            ));
        };
        self.inner.read(b)
    }
}

async fn config(state: &AppState) -> Result<AccountConfig, String> {
    Ok(crate::client(state).await?.account_config().await)
}
#[tauri::command]
pub async fn probe_repositories(
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    folder_ids: Vec<String>,
) -> Result<Vec<Probe>, String> {
    if folder_ids.len() > 32 {
        return Err("请分批检查对象库文件夹".into());
    };
    let c = config(&state).await?;
    let owner = scope(&c);
    let manager = manager.inner().clone();
    let mut rows = tokio::task::spawn_blocking(move || {
        folder_ids
            .into_iter()
            .map(|f| manager.probe(&c, &f))
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|_| "对象库检查已中断")??;
    for p in &mut rows {
        if let Some(id) = &p.repository_id {
            p.saved = system_secrets::exists(&state.store, &secret_id(&owner, id))?
        }
    }
    Ok(rows)
}
#[tauri::command]
pub async fn open_repository(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    folder_id: String,
    repository_id: String,
    unlock: Option<Unlock>,
    use_saved: bool,
    remember: bool,
    request_id: Option<String>,
) -> Result<View, String> {
    if let Some(id)=&request_id {uuid::Uuid::parse_str(id).map_err(|_|"打开请求标识无效")?;}
    let c = config(&state).await?;
    let owner = scope(&c);
    let sid = secret_id(&owner, &repository_id);
    let mut u = if use_saved {
        let b = system_secrets::load(&state.store, Some(&app), &sid)
            .await?
            .ok_or("此对象库没有已保存的解锁信息")?;
        serde_json::from_slice::<Unlock>(&b).map_err(|_| "已保存的解锁信息无效")?
    } else {
        unlock.unwrap_or(Unlock {
            password: String::new(),
            key: String::new(),
            key_file: None,
        })
    };
    manager.materialize(&owner, &mut u)?;
    let sealed = if remember {
        Some(Zeroizing::new(
            serde_json::to_vec(&u).map_err(|_| "解锁信息编码失败")?,
        ))
    } else {
        None
    };
    let m = manager.inner().clone();
    let (handle, actual_sid) =
        tokio::task::spawn_blocking(move || m.open(&c, &folder_id, &repository_id, &u))
            .await
            .map_err(|_| "对象库解锁已中断")??;
    if actual_sid != sid {
        manager.close(&handle);
        return Err("对象库解锁身份不匹配".into());
    };
    if let Some(request_id)=request_id {
        #[derive(Clone,Serialize)]
        #[serde(rename_all="camelCase")]
        struct Opening {request_id:String,handle:String}
        if app.emit("repository-opening",Opening{request_id,handle:handle.clone()}).is_err(){manager.close(&handle);return Err("对象库打开已中断".into())}
    }
    finish_open(
        &app,
        &state,
        manager.inner().clone(),
        handle,
        sid,
        owner,
        sealed,
    )
    .await
}

#[tauri::command]
pub async fn create_repository(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    folder_id: String,
    encrypted: bool,
    unlock: Option<Unlock>,
    remember: bool,
) -> Result<View, String> {
    let c = config(&state).await?;
    let owner = scope(&c);
    let mut u = unlock.unwrap_or(Unlock {
        password: String::new(),
        key: String::new(),
        key_file: None,
    });
    manager.materialize(&owner, &mut u)?;
    if encrypted && u.password.is_empty() && u.key.is_empty() {
        return Err("请设置密码或提供密钥".into());
    };
    if !encrypted && (!u.password.is_empty() || !u.key.is_empty()) {
        return Err("普通对象库不需要解锁信息".into());
    };
    let retain = if remember && encrypted {
        Some(Zeroizing::new(
            serde_json::to_vec(&u).map_err(|_| "解锁信息编码失败")?,
        ))
    } else {
        None
    };
    let m = manager.inner().clone();
    let (handle, sid) =
        tokio::task::spawn_blocking(move || m.create(&c, &folder_id, encrypted, &u))
            .await
            .map_err(|_| "创建已中断；请保留文件夹现有内容后重新检查")??;
    finish_open(
        &app,
        &state,
        manager.inner().clone(),
        handle,
        sid,
        owner,
        retain,
    )
    .await
}

#[tauri::command]
pub async fn create_named_repository(
    app:AppHandle,state:State<'_,AppState>,manager:State<'_,Arc<RepositoryManager>>,
    parent:CloudFile,name:String,encrypted:bool,unlock:Option<Unlock>,remember:bool,
)->Result<View,String>{
    let client=crate::client(&state).await?;let c=client.account_config().await;
    if c.space!="personal_new"{return Err("请在个人云中创建对象库".into())}
    // The cloud client validates the name, original parent and returned folder
    // identity. A partial outcome is retained for inspection, never deleted.
    let owner=scope(&c);let mut u=unlock.unwrap_or(Unlock{password:String::new(),key:String::new(),key_file:None});
    manager.materialize(&owner,&mut u)?;
    let _=key_bytes(&u.key)?;
    if encrypted && u.password.is_empty() && u.key.is_empty(){return Err("请先设置密码或提供密钥".into())}
    if !encrypted && (!u.password.is_empty() || !u.key.is_empty()){return Err("普通对象库不需要解锁信息".into())}
    let folder=client.create_directory(&parent,&name).await?;
    let retain=if remember&&encrypted{Some(Zeroizing::new(serde_json::to_vec(&u).map_err(|_|"解锁信息编码失败")?))}else{None};
    let m=manager.inner().clone();let (handle,sid)=tokio::task::spawn_blocking(move||m.create(&c,&folder.id,encrypted,&u)).await.map_err(|_|"创建已中断，请检查新文件夹")??;
    finish_open(&app,&state,manager.inner().clone(),handle,sid,owner,retain).await
}

#[derive(Serialize)]
pub struct UnlockChange {view:View,notice:Option<String>}
#[tauri::command]
pub async fn change_repository_unlock(
    app:AppHandle,state:State<'_,AppState>,manager:State<'_,Arc<RepositoryManager>>,
    handle:String,revision:String,mut unlock:Unlock,remember:bool,
)->Result<UnlockChange,String>{
    let owner=scope(&config(&state).await?);manager.materialize(&owner,&mut unlock)?;
    if unlock.password.is_empty()==unlock.key.is_empty(){return Err("每次请选择一种新的解锁方式".into())}
    let retained=if remember{Some(Zeroizing::new(serde_json::to_vec(&unlock).map_err(|_|"解锁信息编码失败")?))}else{None};
    let session=manager.session(&handle,&owner)?;
    let checkpoint_directory=manager.directory.clone();let checkpoint_owner=owner.clone();
    let repo_id=tokio::task::spawn_blocking(move||->Result<String,String>{
        let key=key_bytes(&unlock.key)?;let mut state=session.state.lock().map_err(|_|"对象库正在操作")?;
        let checkpoint=checkpoint_directory.join(format!("{}-{}.unlock-change.json",checkpoint_owner,state.repo.descriptor.id));
        state.repo.change_unlock_with_checkpoint(&revision,(!unlock.password.is_empty()).then_some(unlock.password.as_str()),key.as_ref().map(|v|&**v),&checkpoint,&session.observation).map_err(|e|{
            if checkpoint.exists() && matches!(e,sdk::Error::Recovery|sdk::Error::Io(_)|sdk::Error::Incomplete){"此次凭据更新尚未确认，请使用同一份新凭据重试；原更新检查点已保留。".into()}else{error(e)}
        })?;
        Ok(state.repo.descriptor.id.clone())
    }).await.map_err(|_|"解锁方式更新已中断，请重新打开管理页面核对")??;
    let sid=secret_id(&owner,&repo_id);let mut notice=None;
    if system_secrets::remove(&state.store,&sid).is_err(){notice=Some("云端解锁方式已更新，但本机旧记录未能清除，请在管理页面重试清除。".into())}
    if let Some(bytes)=retained{if system_secrets::save(&state.store,Some(&app),&sid,&bytes).await.is_err(){notice=Some("云端解锁方式已更新，本机未能保存新凭据。请妥善保管新的密码或密钥。".into())}}
    let saved=system_secrets::exists(&state.store,&sid)?;let m=manager.inner().clone();
    let view=tokio::task::spawn_blocking(move||m.view(&handle,&owner,"",saved,false)).await.map_err(|_|"管理页面读取已中断")??;
    Ok(UnlockChange{view,notice})
}

#[tauri::command]
pub async fn generate_repository_key(app:AppHandle,state:State<'_,AppState>,manager:State<'_,Arc<RepositoryManager>>)->Result<Option<PickedKey>,String>{
    use rand::RngCore;
    let owner=scope(&config(&state).await?);let mut key=Zeroizing::new([0u8;32]);rand::rngs::OsRng.fill_bytes(&mut *key);
    let bytes=Zeroizing::new(format!("AXUANSTOR-KEY-v1:{}\n",STANDARD_NO_PAD.encode(&*key)));
    let name="AxuanStor.key".to_string();
    #[cfg(target_os="android")]
    let exported={
        let dir=manager.cache.join("key-exports");std::fs::create_dir_all(&dir).map_err(|_|"密钥暂存目录不可用")?;
        let path=dir.join(format!("AxuanStor-{}.key",uuid::Uuid::new_v4()));
        sdk::local_state::write(&path,bytes.as_bytes()).map_err(error)?;
        let result=app.document_picker().export_download(path.to_str().ok_or("密钥文件路径无效")?).await;
        std::fs::remove_file(&path).map_err(|_|"临时密钥文件未能清除，请检查应用缓存")?;
        result?
    };
    #[cfg(not(target_os="android"))]
    let exported={
        let picker=app.clone();let destination=tokio::task::spawn_blocking(move||picker.dialog().file().set_title("保存新密钥").set_file_name("AxuanStor.key").blocking_save_file()).await.map_err(|_|"密钥保存已中断")?;
        if let Some(path)=destination{
            let mut f=app.fs().open(path,OpenOptions::new().write(true).create(true).truncate(true).clone()).map_err(|_|"无法写入所选密钥文件")?;
            f.write_all(bytes.as_bytes()).map_err(|_|"密钥文件保存失败")?;f.sync_all().map_err(|_|"密钥文件保存失败")?;true
        }else{false}
    };
    if !exported{return Ok(None)};
    let selection_id=uuid::Uuid::new_v4().to_string();let mut keys=manager.keys.lock().map_err(|_|"密钥选择已中断")?;
    if keys.len()>=32{keys.clear()};keys.insert(selection_id.clone(),ImportedKey{scope:owner,key});
    Ok(Some(PickedKey{selection_id,name}))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedKey {
    selection_id: String,
    name: String,
}
#[tauri::command]
pub async fn import_repository_key(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
) -> Result<Option<PickedKey>, String> {
    let owner = scope(&config(&state).await?);
    #[cfg(target_os = "android")]
    let paths: Vec<(String, String)> = app
        .document_picker()
        .pick_files()
        .await?
        .into_iter()
        .map(|d| (d.path, d.name))
        .collect();
    #[cfg(not(target_os = "android"))]
    let paths: Vec<(String, String)> = {
        let p = app.clone();
        tokio::task::spawn_blocking(move || {
            p.dialog()
                .file()
                .set_title("选择 AxuanStor 密钥文件")
                .blocking_pick_file()
        })
        .await
        .map_err(|_| "密钥文件选择已中断")?
        .into_iter()
        .map(|p| {
            let n = p
                .as_path()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("密钥文件")
                .to_string();
            (p.to_string(), n)
        })
        .collect()
    };
    if paths.is_empty() {
        return Ok(None);
    };
    if paths.len() != 1 {
        return Err("请选择一个密钥文件".into());
    };
    let (path, name) = paths.into_iter().next().unwrap();
    let m = manager.inner().clone();
    tokio::task::spawn_blocking(move || {
        let f = app
            .fs()
            .open(
                FilePath::from_str(&path).map_err(|_| "密钥文件来源无效")?,
                OpenOptions::new().read(true).clone(),
            )
            .map_err(|_| "无法打开所选密钥文件")?;
        let mut b = Zeroizing::new(vec![]);
        f.take(4097)
            .read_to_end(&mut b)
            .map_err(|_| "密钥文件读取失败")?;
        if b.len() > 4096 {
            return Err("密钥文件过大".into());
        };
        let key = if b.len() == 32 {
            Zeroizing::new(b.as_slice().try_into().map_err(|_| "密钥格式无效")?)
        } else {
            key_bytes(std::str::from_utf8(&b).map_err(|_| "密钥文件格式无效")?)?
                .ok_or("密钥文件为空")?
        };
        let selection_id = uuid::Uuid::new_v4().to_string();
        let mut keys = m.keys.lock().map_err(|_| "密钥选择已中断")?;
        if keys.len() >= 32 {
            keys.clear()
        };
        keys.insert(selection_id.clone(), ImportedKey { scope: owner, key });
        Ok(Some(PickedKey { selection_id, name }))
    })
    .await
    .map_err(|_| "密钥导入已中断")?
}
#[tauri::command]
pub async fn list_repository(
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    directory: String,
    refresh: Option<bool>,
) -> Result<View, String> {
    let c = config(&state).await?;
    let owner = scope(&c);
    let session = manager.session(&handle, &owner)?;
    let repo_id = session
        .state
        .lock()
        .map_err(|_| "对象库会话不可用")?
        .repo
        .descriptor
        .id
        .clone();
    let saved = system_secrets::exists(&state.store, &secret_id(&owner, &repo_id))?;
    let m = manager.inner().clone();
    tokio::task::spawn_blocking(move || m.view(&handle, &owner, &directory, saved, refresh.unwrap_or(false)))
        .await
        .map_err(|_| "对象库读取已中断")?
}
#[tauri::command]
pub async fn close_repository(
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
) -> Result<(), String> {
    manager.close(&handle);
    Ok(())
}

#[tauri::command]
pub async fn forget_repository_unlock(
    state: State<'_, AppState>,
    repository_id: String,
) -> Result<(), String> {
    let c = config(&state).await?;
    system_secrets::remove(&state.store, &secret_id(&scope(&c), &repository_id))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edit {
    kind: String,
    path: String,
    revision: String,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    paths: Vec<String>,
}
#[tauri::command]
pub async fn edit_repository(
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    edit: Edit,
) -> Result<(), String> {
    let owner = scope(&config(&state).await?);
    let s = manager.session(&handle, &owner)?;
    tokio::task::spawn_blocking(move || {
        let mut state = s.state.lock().map_err(|_| "对象库正在操作")?;
        if state.view.is_none(){return Err("请先读取对象库目录".into())}
        let writer = digest(owner.as_bytes())[..32].to_string();
        if !edit.paths.is_empty() {
            if edit.paths.len() > 10000 { return Err("单次操作项目过多，请分批选择".into()); }
            let mut paths = edit.paths.iter().map(|path| logical_path(path).map_err(error)).collect::<Result<Vec<_>,String>>()?;
            paths.sort();paths.dedup();
            let selected:HashSet<&str>=paths.iter().map(String::as_str).collect();
            let roots:Vec<String>=paths.iter().filter(|path|!path.match_indices('/').any(|(end,_)|selected.contains(&path[..end]))).cloned().collect();
            if edit.kind == "remove" {
                let (basis, _) = scoped_write_state(&s, &mut state, &[], &roots)?;
                if state.sync_revision != edit.revision { return Err("目录视图已更新，请刷新后重新确认操作".into()); }
                state.repo.remove_many(&basis,&writer,&roots).map_err(error)?;
            } else if matches!(edit.kind.as_str(),"copy"|"move") {
                let destination = if edit.destination.is_empty() {String::new()} else {logical_path(&edit.destination).map_err(error)?};
                let moves = roots.iter().map(|path| {
                    let name = path.rsplit('/').next().ok_or("文件名称无效")?;
                    Ok((path.clone(),if destination.is_empty(){name.into()}else{format!("{destination}/{name}")}))
                }).collect::<Result<Vec<_>,String>>()?;
                let dest_witness: Vec<String> = moves.iter().map(|(_, dst)| dst.clone()).collect();
                let (basis, _) = scoped_write_state(&s, &mut state, &dest_witness, &roots)?;
                if state.sync_revision != edit.revision { return Err("目录视图已更新，请刷新后重新确认操作".into()); }
                state.repo.relocate_many(&basis,&writer,&moves,edit.kind=="move").map_err(error)?;
            } else {return Err("未知对象库批量操作".into())}
            state.view=None;
            return state.repo.save_observed_file(&s.observation).map_err(error)
        }
        let (witness, subtrees): (Vec<String>, Vec<String>) = match edit.kind.as_str() {
            "mkdir" => (vec![edit.path.clone()], vec![]),
            "remove" if !edit.recursive => (vec![edit.path.clone()], vec![]),
            "remove" => (vec![], vec![edit.path.clone()]),
            "rename" | "move" | "copy" => (vec![edit.destination.clone()], vec![edit.path.clone()]),
            _ => return Err("未知对象库操作".into()),
        };
        let (basis, _) = scoped_write_state(&s, &mut state, &witness, &subtrees)?;
        let current=state.sync_revision.clone();
        if current!=edit.revision{return Err("目录视图已更新，请刷新后重新确认操作".into())}
        match edit.kind.as_str() {
            "mkdir" => state.repo.mkdir(&basis, &writer, &edit.path),
            "rename" | "move" => state
                .repo
                .rename(&basis, &writer, &edit.path, &edit.destination),
            "copy" => state.repo.copy(&basis, &writer, &edit.path, &edit.destination),
            "remove" => state
                .repo
                .remove(&basis, &writer, &edit.path, edit.recursive),
            _ => return Err("未知对象库操作".into()),
        }
        .map_err(error)?;
        state.view = None;
        state.repo.save_observed_file(&s.observation).map_err(error)
    })
    .await
    .map_err(|_| "对象库操作已中断")?
}

#[tauri::command]
pub async fn plan_repository_uploads(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    directory: String,
    mode: Option<String>,
    paths: Option<Vec<String>>,
) -> Result<Option<Batch>, String> {
    let owner = scope(&config(&state).await?);
    let session = manager.session(&handle, &owner)?;
    let _scan=state.scan.try_lock().map_err(|_|"已有文件夹扫描，请完成或取消后重试")?;
    state.scan_cancel.store(false,Ordering::Release);
    let Some(selection)=sources::select(&app,&state,mode.as_deref()==Some("directory"),paths).await? else{return Ok(None)};
    let mut directories=selection.directories.into_iter().map(|path|logical_path(&if directory.is_empty(){path}else{format!("{directory}/{path}")}).map_err(error)).collect::<Result<Vec<_>,String>>()?;
    directories.sort();directories.dedup();let skipped=selection.skipped;let source_root=selection.root;let selected=selection.files;
    tokio::task::spawn_blocking(move || {
        let mut sources = vec![];
        let local_paths=selected.iter().map(|row|row.0.clone()).collect::<Vec<_>>();
        for (path, name, size) in selected {
            let logical = logical_path(&if directory.is_empty() {
                name
            } else {
                format!("{directory}/{name}")
            })
            .map_err(error)?;
            let source = FilePath::from_str(&path).map_err(|_| "文件来源无效")?;
            let file = app
                .fs()
                .open(source, OpenOptions::new().read(true).clone())
                .map_err(|_| "无法读取所选文件")?;
            let metadata = file.metadata();
            let size = match size {
                Some(size) => size,
                None => metadata.as_ref().map_err(|_| "无法取得文件大小")?.len(),
            };
            let modified_at = metadata
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(sdk::repository::file_modified_at_from_system_time);
            drop(file);
            let app = app.clone();
            let cancel = session.backend.cancel_flag();
            sources.push(UploadSource {
                path: logical,
                size,
                modified_at,
                open: Arc::new(move || {
                    let p = FilePath::from_str(&path).map_err(|_| sdk::Error::Invalid)?;
                    let f = app
                        .fs()
                        .open(p, OpenOptions::new().read(true).clone())
                        .map_err(|_| sdk::Error::Incomplete)?;
                    Ok(Box::new(CancelReader {
                        inner: f,
                        cancel: cancel.clone(),
                    }))
                }),
            });
        }
        let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
        if sources.iter().any(|s| {
            state
                .repo
                .strong_policy()
                .is_some_and(|p| p.protects(&s.path))
        }) {
            return Err("所选位置需要已登记设备的目录授权，请先完成目录授权".into());
        };
        let mut witness: Vec<String> = sources.iter().map(|s| s.path.clone()).collect();
        witness.extend(directories.iter().cloned());
        if !directory.is_empty() {
            witness.push(directory.clone());
        }
        let (basis, used_compact) = scoped_write_state(&session, &mut state, &witness, &[])?;
        let plan = state.repo.plan_uploads_scoped(&basis, &sources, used_compact).map_err(error)?;
        let id = uuid::Uuid::new_v4().to_string();
        let output = Batch {
            id: id.clone(),
            plan: plan.clone(),
            directories:directories.len(),
            skipped,
        };
        state.pending = Some(Pending {
            id,
            basis,
            sources,
            plan,
            local_paths,
            queued:false,
            directories,
            source_root,
        });
        Ok(Some(output))
    })
    .await
    .map_err(|_| "上传比对已中断")?
}
#[tauri::command]
pub async fn execute_repository_uploads(
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    plan_id: String,
    replace: bool,
) -> Result<Vec<crate::transfers::TransferJob>, String> {
    let owner=scope(&config(&state).await?);let session=manager.session(&handle,&owner)?;
    let s=session.clone();let expected=plan_id.clone();
    let (spec,intent)=tokio::task::spawn_blocking(move||{
        let mut state=s.state.lock().map_err(|_|"对象库正在操作")?;let encrypted=state.repo.descriptor.kind=="encrypted";
        let pending=state.pending.as_mut().ok_or("上传比对已过期，请重新选择文件")?;
        if pending.id!=expected||pending.queued{return Err("上传比对已处理，请刷新传输列表".into())}
        let draft=queue::upload_draft(&s,pending,replace,encrypted)?;pending.queued=true;Ok::<_,String>(draft)
    }).await.map_err(|_|"上传入队已中断")??;
    let result=state.transfers.enqueue_with_repository(vec![spec],vec![Some(intent)]).await;
    if let Ok(mut state)=session.state.lock(){if state.pending.as_ref().is_some_and(|pending|pending.id==plan_id){if result.is_ok(){state.pending=None}else if let Some(pending)=state.pending.as_mut(){pending.queued=false}}}
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    id: String,
    name: String,
    size: u64,
    needs_export: bool,
}
#[derive(Serialize)]
pub struct FileLink{url:String,encrypted:bool}
#[tauri::command]
pub async fn repository_file_link(state:State<'_,AppState>,manager:State<'_,Arc<RepositoryManager>>,handle:String,revision:String,path:String)->Result<FileLink,String>{
    let owner=scope(&config(&state).await?);let session=manager.session(&handle,&owner)?;
    tokio::task::spawn_blocking(move||{
        let path=logical_path(&path).map_err(error)?;
        let mut state=session.state.lock().map_err(|_|"对象库正在操作")?;
        let view=viewed_file_basis(&session,&mut state,&revision,&path)?;
        let entry=view.resolve(&path).map_err(error)?;
        if entry.kind!="file"{return Err("请选择一个文件".into())}
        let reference=entry.object.clone().ok_or("文件引用不完整")?;let encrypted=state.repo.descriptor.kind=="encrypted";drop(state);
        let url=session.backend.download_url(&reference).map_err(error)?;Ok(FileLink{url,encrypted})
    }).await.map_err(|_|"下载链接读取已中断")?
}
#[tauri::command]
pub async fn download_repository_file(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    revision: String,
    path: String,
) -> Result<Option<Download>, String> {
    let owner = scope(&config(&state).await?);
    let session = manager.session(&handle, &owner)?;
    #[cfg(not(target_os = "android"))]
    let directory = {
        let app = app.clone();
        let selected = tokio::task::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_title("选择保存文件夹")
                .blocking_pick_folder()
        })
        .await
        .map_err(|_| "文件夹选择已中断")?;
        let Some(selected) = selected else {
            return Ok(None);
        };
        selected.into_path().map_err(|_| "保存位置无效")?
    };
    #[cfg(target_os = "android")]
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "下载目录不可用")?
        .join("Downloads")
        .join("AxuanStor")
        .join(uuid::Uuid::new_v4().to_string());
    let m = manager.inner().clone();
    tokio::task::spawn_blocking(move || {
        let state = session.state.lock().map_err(|_| "对象库正在操作")?;
        let (shown, basis) = state.view.as_ref().ok_or("请先读取对象库目录")?;
        if shown != &revision {
            return Err("目录视图已更新，请重新选择文件".into());
        };
        let entry = basis.resolve(&path).map_err(error)?;
        if entry.kind != "file" {
            return Err("请选择一个文件".into());
        };
        let name = path.rsplit('/').next().ok_or("文件名称无效")?.to_string();
        crate::transfers::validate_filename(&name)?;
        std::fs::create_dir_all(&directory).map_err(|_| "保存目录不可用")?;
        let output = directory.join(&name);
        state
            .repo
            .export_file(basis, &path, &output, 2 << 30)
            .map_err(error)?;
        let id = uuid::Uuid::new_v4().to_string();
        m.downloads
            .lock()
            .map_err(|_| "取回记录不可用")?
            .insert(id.clone(), output);
        Ok(Some(Download {
            id,
            name,
            size: entry.size,
            needs_export: cfg!(target_os = "android"),
        }))
    })
    .await
    .map_err(|_| "取回已中断，目标文件未发布")?
}
#[tauri::command]
pub async fn export_repository_download(
    app: AppHandle,
    manager: State<'_, Arc<RepositoryManager>>,
    id: String,
) -> Result<bool, String> {
    let path = manager
        .downloads
        .lock()
        .map_err(|_| "取回记录不可用")?
        .get(&id)
        .cloned()
        .ok_or("取回记录不存在")?;
    #[cfg(target_os = "android")]
    {
        app.document_picker()
            .export_download(path.to_str().ok_or("文件路径无效")?)
            .await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, path);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicit read-only acceptance against a caller-selected original repo.
    /// Uses a separate credential file and local state, never the app vault.
    #[test]
    #[ignore = "requires CMCLOUD_AXUANSTOR_LIVE with private, explicitly selected provider evidence"]
    fn original_repository_native_read_only() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Live {
            credentials_file: PathBuf,
            state_directory: PathBuf,
            folder_id: String,
            repository_id: String,
            expected_paths: usize,
            sample_path: String,
            sample_object_id: String,
        }
        let file = PathBuf::from(std::env::var("CMCLOUD_AXUANSTOR_LIVE").expect("explicit private evidence required"));
        let raw = sdk::local_state::read(&file, 65536).expect("private evidence unavailable");
        let live: Live = serde_json::from_slice(&raw).expect("invalid evidence shape");
        let credential = Zeroizing::new(sdk::local_state::read(&live.credentials_file, 65536).expect("private provider credential unavailable"));
        let value: serde_json::Value = serde_json::from_slice(&credential).expect("invalid private credential shape");
        assert_eq!(value["format"], 1);
        assert_eq!(value["provider"], "cmcloud");
        let mut account = AccountConfig { id: "read-only-native-acceptance".into(), authorization: value["authorization"].as_str().expect("provider authority missing").into(), ..Default::default() };
        let manager = RepositoryManager::new(live.state_directory.clone(), live.state_directory.join("cache")).expect("separate local state unavailable");
        let owner = scope(&account);
        let factory_id = digest(format!("{owner}\0{}", account.authorization).as_bytes());
        let factory = MobileCloud::new(account.authorization.clone(), live.folder_id.clone(), None).expect("provider initialization failed").with_download_budget(sdk::rate::Budget::new(1 << 20).unwrap());
        manager.factories.lock().unwrap().push((factory_id, Arc::new(factory)));
        let probe = manager.probe(&account, &live.folder_id).expect("repository discovery failed");
        assert_eq!(probe.state, "repository");
        assert_eq!(probe.repository_id.as_deref(), Some(live.repository_id.as_str()));
        let unlock = Unlock { password: String::new(), key: String::new(), key_file: None };
        let (handle, _) = manager.open(&account, &live.folder_id, &live.repository_id, &unlock).expect("plain repository open failed");
        let directory = live.sample_path.rsplit_once('/').unwrap().0;
        let view = manager.view(&handle, &owner, directory, false, false).expect("native directory navigation failed");
        assert_eq!(view.repository_id, live.repository_id);
        assert!(!view.encrypted);
        assert!(!view.saved);
        assert!(view.conflicts.is_empty());
        assert!(!view.items.is_empty());
        let session = manager.session(&handle, &owner).unwrap();
        {
            let mut state = session.state.lock().unwrap();
            let (_, snapshot) = state.view.as_ref().unwrap();
            let entry = snapshot.resolve(&live.sample_path).expect("original logical path missing").clone();
            // A displayed directory deliberately contains only its final child state.
            // Full-scope acceptance remains an explicit SDK check, not a GUI open cost.
            let full=state.repo.refresh().expect("full-scope SDK acceptance failed");
            assert!(full.versions.len() >= live.expected_paths);
            assert_eq!(entry.object.as_ref().unwrap().id, live.sample_object_id);
        }
        manager.close(&handle);
        account.authorization.zeroize();
    }
    #[test]
    #[ignore = "explicit private metadata-only live acceptance; requires reserved physical bandwidth"]
    fn original_repository_persistent_directory_read_only(){
        use std::time::Instant;
        #[derive(Deserialize)]
        #[serde(rename_all="camelCase",deny_unknown_fields)]
        struct Live{credentials_file:PathBuf,state_directory:PathBuf,folder_id:String,repository_id:String}
        let path=PathBuf::from(std::env::var("CMCLOUD_DIRECTORY_LIVE").expect("explicit live evidence required"));
        let live:Live=serde_json::from_slice(&sdk::local_state::read(&path,65536).expect("private live evidence unavailable")).expect("invalid live evidence");
        let credentials=Zeroizing::new(sdk::local_state::read(&live.credentials_file,65536).expect("independent private provider unavailable"));
        let authority:serde_json::Value=serde_json::from_slice(&credentials).expect("invalid independent provider format");
        assert!(authority["format"]==1&&authority["provider"]=="cmcloud");
        let mut account=AccountConfig{id:"persistent-directory-native-acceptance".into(),authorization:authority["authorization"].as_str().expect("provider authority missing").into(),..Default::default()};
        let manager=RepositoryManager::new(live.state_directory.clone(),live.state_directory.join("cache")).unwrap();
        let owner=scope(&account);let factory_id=digest(format!("{owner}\0{}",account.authorization).as_bytes());
        let cloud=MobileCloud::new(account.authorization.clone(),live.folder_id.clone(),None).expect("native provider initialization failed").with_download_budget(sdk::rate::Budget::new(1<<20).unwrap());
        manager.factories.lock().unwrap().push((factory_id,Arc::new(cloud)));
        let unlock=Unlock{password:String::new(),key:String::new(),key_file:None};
        let start=Instant::now();
        let(handle,_)=manager.open(&account,&live.folder_id,&live.repository_id,&unlock).expect("native original repository open failed");
        let root=manager.view(&handle,&owner,"",false,false).expect("native root directory failed");
        let first_ms=start.elapsed().as_millis();assert!(!root.items.is_empty()&&!root.encrypted&&root.repository_id==live.repository_id);
        let start=Instant::now();let directory="1.7.1300/cn/android/Packages";
        let child=manager.view(&handle,&owner,directory,false,false).expect("native deep directory failed");let child_ms=start.elapsed().as_millis();
        let sample=child.items.iter().find(|i|i.item.kind=="file"&&!i.item.conflict).expect("original file entry missing");
        let session=manager.session(&handle,&owner).unwrap();
        let cached=DirectoryIndex::open(&session.index_directory,&session.state.lock().unwrap().repo).expect("persistent GUID index missing");
        assert!(cached.path().is_file());
        drop(cached);
        {
            let state=session.state.lock().unwrap();let view=&state.view.as_ref().unwrap().1;
            let entry=view.resolve(&sample.item.path).expect("original entry missing from final directory state");
            assert!(entry.object.as_ref()==Some(&session.backend.stat(entry.object.as_ref().unwrap()).expect("original FileID metadata unavailable")));
            assert!(view.versions.len()>=5000&&view.versions.len()<20000);
        }
        drop(session);manager.close(&handle);
        let start=Instant::now();let(handle,_)=manager.open(&account,&live.folder_id,&live.repository_id,&unlock).unwrap();
        let reopened=manager.view(&handle,&owner,"",false,false).expect("persistent root reopen failed");let reopen_ms=start.elapsed().as_millis();
        assert!(reopened.items.len()==root.items.len());
        let session=manager.session(&handle,&owner).unwrap();assert!(session.state.lock().unwrap().view.as_ref().unwrap().1.versions.len()<100);
        manager.close(&handle);account.authorization.zeroize();
        let result=serde_json::json!({"firstRootMS":first_ms,"directoryMS":child_ms,"persistentReopenMS":reopen_ms,"rootItems":root.items.len(),"directoryItems":child.items.len(),"repository":live.repository_id,"packageBodiesRead":0,"downloadLimitBPS":1<<20,"providerMetadataIdentityVerified":true,"vaultUsed":false});
        sdk::local_state::write(&live.state_directory.join("native-directory-result.json"),&serde_json::to_vec_pretty(&result).unwrap()).unwrap();println!("{result}");
    }
    #[test]
    fn cancelled_sources_stop_without_interrupted_retry_loops() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut source = CancelReader {
            inner: std::io::Cursor::new(b"bytes"),
            cancel: cancel.clone(),
        };
        assert_eq!(source.read(&mut [0; 1]).unwrap(), 1);
        cancel.store(true, Ordering::Release);
        assert_eq!(
            source.read(&mut [0; 1]).unwrap_err().kind(),
            std::io::ErrorKind::ConnectionAborted
        );
    }
    #[test]
    fn unlock_inputs_support_only_exact_key_material() {
        let key = [41u8; 32];
        for s in [
            hex::encode(key),
            STANDARD_NO_PAD.encode(key),
            format!("AXUANSTOR-KEY-v1:{}", STANDARD_NO_PAD.encode(key)),
        ] {
            assert_eq!(**key_bytes(&s).unwrap().as_ref().unwrap(), key)
        }
        for bad in ["password", "AXUANSTOR-KEY-v1:", "../key"] {
            assert!(key_bytes(bad).is_err())
        }
    }
    #[test]
    fn errors_and_secret_ids_do_not_expose_authorities() {
        let c = AccountConfig {
            id: "fixture".into(),
            authorization: "PRIVATE_AUTHORITY".into(),
            ..Default::default()
        };
        let id = secret_id(&scope(&c), "fixture-repo");
        assert!(!id.contains("PRIVATE"));
        assert!(error(sdk::Error::Io(std::io::Error::other(
            "https://private/token"
        )))
        .find("private")
        .is_none());
        assert_ne!(id, secret_id(&scope(&c), "different-repo"))
    }
}
