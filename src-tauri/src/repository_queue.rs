//! Repository payloads use the ordinary durable transfer scheduler. Only
//! immutable references and picker-approved paths are persisted, under the
//! account vault's encryption; unlock material is never part of a job.
use super::*;
use crate::transfers::{Control, TransferDirection, TransferJob, TransferManager, TransferSpec};
use sdk::repository::ObjectRef;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryTransfer {
    #[serde(default)]
    pub(super) preview: bool,
    format: u32,
    owner: String,
    folder: String,
    repository: String,
    encrypted: bool,
    basis: Snapshot,
    paths: Vec<String>,
    local_paths: Vec<String>,
    directories: Vec<String>,
    source_root: String,
    plan: Option<UploadPlan>,
    replace: bool,
    download_root: Option<PathBuf>,
}
impl RepositoryTransfer {
    pub(crate) fn preview_path(&self) -> Option<&str> {
        if self.preview && self.plan.is_none() && self.local_paths.len() == 1 {
            self.local_paths.first().map(String::as_str)
        } else {
            None
        }
    }
    pub(crate) fn queue_size_name(&self, spec: &TransferSpec) -> Result<(String, u64), String> {
        self.validate()?;
        if spec.remote.id != format!("axuanstor:{}", self.repository)
            || self.source_root != spec.local_path
        {
            return Err("对象库传输身份不一致".into());
        }
        if let Some(plan) = &self.plan {
            if spec.direction != TransferDirection::Upload {
                return Err("对象库传输方向不一致".into());
            }
            Ok((
                if plan.items.len() == 1 && self.directories.is_empty() {
                    plan.items[0]
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or("上传文件")
                        .into()
                } else {
                    format!(
                        "{} 个文件 · {} 个文件夹",
                        plan.items.len(),
                        self.directories.len()
                    )
                },
                plan.logical_bytes,
            ))
        } else {
            if spec.direction != TransferDirection::Download || self.paths.len() != 1 {
                return Err("对象库下载任务无效".into());
            }
            let entry = self.basis.resolve(&self.paths[0]).map_err(error)?;
            Ok((
                self.paths[0].rsplit('/').next().ok_or("文件名无效")?.into(),
                entry.size,
            ))
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.format != 1
            || self.owner.len() != 64
            || self.repository != self.basis.repository
            || self.repository.len() != 32
            || (self.paths.is_empty() && self.directories.is_empty())
            || self.paths.len() + self.directories.len() > 10000
            || self.paths.len() != self.local_paths.len()
            || self.basis.heads.len() > 1024
            || self.folder.is_empty()
            || self.source_root.is_empty()
        {
            return Err("对象库传输断点身份无效".into());
        }
        for path in &self.paths {
            logical_path(path).map_err(error)?;
        }
        for path in &self.directories {
            logical_path(path).map_err(error)?;
        }
        if let Some(plan) = &self.plan {
            if plan.items.len() != self.paths.len()
                || plan
                    .items
                    .iter()
                    .zip(&self.paths)
                    .any(|(item, path)| &item.path != path)
            {
                return Err("对象库上传清单不一致".into());
            }
        } else if self
            .download_root
            .as_ref()
            .is_none_or(|root| !root.is_absolute())
        {
            return Err("对象库保存目录无效".into());
        }
        Ok(())
    }
}
fn cloud_file(repository: &str, name: String, size: u64) -> CloudFile {
    CloudFile {
        id: format!("axuanstor:{repository}"),
        name,
        path: String::new(),
        is_dir: false,
        size,
        modified_at: String::new(),
        thumbnail: None,
        source_id: None,
    }
}
fn narrowed(basis: &Snapshot, paths: &[String], plan: Option<&UploadPlan>) -> Snapshot {
    let selected: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let wanted: HashSet<ObjectRef> = plan
        .into_iter()
        .flat_map(|plan| &plan.items)
        .filter_map(|item| item.existing.clone())
        .collect();
    let mut found = HashSet::new();
    let mut versions = BTreeMap::new();
    for (path, rows) in &basis.versions {
        let needed = selected.contains(path.as_str())
            || rows.iter().any(|row| {
                row.entry
                    .as_ref()
                    .and_then(|entry| entry.object.as_ref())
                    .is_some_and(|reference| {
                        wanted.contains(reference) && found.insert(reference.clone())
                    })
            });
        if needed {
            versions.insert(path.clone(), rows.clone());
        }
    }
    Snapshot {
        repository: basis.repository.clone(),
        heads: basis.heads.clone(),
        versions,
        conflicts: basis
            .conflicts
            .iter()
            .filter(|path| selected.contains(path.as_str()))
            .cloned()
            .collect(),
    }
}
#[derive(Default)]
struct DownloadLayout {
    paths: HashMap<String, String>,
    used: HashSet<String>,
}
impl DownloadLayout {
    fn allocate(
        &mut self,
        remote: &str,
        parent: &str,
        name: &str,
        directory: bool,
        root: &std::path::Path,
    ) -> Result<String, String> {
        if let Some(path) = self.paths.get(remote) {
            return Ok(path.clone());
        }
        let safe = crate::transfers::sanitize_filename(name);
        let file = std::path::Path::new(&safe);
        let stem = if directory {
            safe.as_str()
        } else {
            file.file_stem().and_then(|v| v.to_str()).unwrap_or(&safe)
        };
        let ext = if directory {
            None
        } else {
            file.extension().and_then(|v| v.to_str())
        };
        for n in 0..=10000 {
            let name = if n == 0 {
                safe.clone()
            } else {
                format!(
                    "{stem} ({n}){}",
                    ext.map(|v| format!(".{v}")).unwrap_or_default()
                )
            };
            let relative = if parent.is_empty() {
                name
            } else {
                format!("{parent}/{name}")
            };
            let output = root.join(&relative);
            let occupied = std::fs::symlink_metadata(&output)
                .is_ok_and(|meta| !directory || cache::reparse(&meta) || !meta.is_dir());
            if !occupied && self.used.insert(relative.to_lowercase()) {
                self.paths.insert(remote.into(), relative.clone());
                return Ok(relative);
            }
        }
        Err("同名文件过多，请选择另一个保存目录".into())
    }
}
pub(super) fn upload_draft(
    session: &Session,
    pending: &Pending,
    replace: bool,
    encrypted: bool,
) -> Result<(TransferSpec, RepositoryTransfer), String> {
    if pending
        .plan
        .items
        .iter()
        .any(|item| item.action.starts_with("replace-"))
        && !replace
    {
        return Err("请先确认替换所列路径".into());
    }
    let paths = pending
        .sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<Vec<_>>();
    let all = paths
        .iter()
        .chain(&pending.directories)
        .cloned()
        .collect::<Vec<_>>();
    let intent = RepositoryTransfer {
        preview: false,
        format: 1,
        owner: session.scope.clone(),
        folder: session.folder.clone(),
        repository: pending.basis.repository.clone(),
        encrypted,
        basis: narrowed(&pending.basis, &all, Some(&pending.plan)),
        paths,
        local_paths: pending.local_paths.clone(),
        directories: pending.directories.clone(),
        source_root: pending.source_root.clone(),
        plan: Some(pending.plan.clone()),
        replace,
        download_root: None,
    };
    let spec = TransferSpec {
        direction: TransferDirection::Upload,
        remote: cloud_file(
            &intent.repository,
            "对象库上传".into(),
            pending.plan.logical_bytes,
        ),
        local_path: intent.source_root.clone(),
        name: None,
    };
    intent.queue_size_name(&spec)?;
    Ok((spec, intent))
}

#[tauri::command]
pub async fn queue_repository_downloads(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    revision: String,
    paths: Vec<String>,
) -> Result<Vec<TransferJob>, String> {
    let owner = scope(&config(&state).await?);
    let session = manager.session(&handle, &owner)?;
    if paths.is_empty() || paths.len() > 10000 {
        return Err("请选择 1 至 10000 项".into());
    }
    #[cfg(not(target_os = "android"))]
    let directory = {
        let a = app.clone();
        let selected = tokio::task::spawn_blocking(move || {
            a.dialog()
                .file()
                .set_title("选择保存文件夹")
                .blocking_pick_folder()
        })
        .await
        .map_err(|_| "文件夹选择已中断")?;
        let Some(selected) = selected else {
            return Ok(vec![]);
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
    let (specs, intents) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
        let selected = paths
            .iter()
            .map(|path| logical_path(path).map_err(error))
            .collect::<Result<HashSet<_>, _>>()?;
        let mut all_sorted = selected.iter().cloned().collect::<Vec<_>>();
        all_sorted.sort();
        let roots: Vec<String> = all_sorted
            .into_iter()
            .filter(|path| {
                let mut parent = path.rsplit_once('/').map(|v| v.0);
                while let Some(p) = parent {
                    if selected.contains(p) {
                        return false;
                    }
                    parent = p.rsplit_once('/').map(|v| v.0);
                }
                true
            })
            .collect();
        let direct_view_files = state.view.as_ref().and_then(|(shown, view_basis)| {
            if shown == &revision
                && roots
                    .iter()
                    .all(|p| view_basis.resolve(p).is_ok_and(|e| e.kind == "file"))
            {
                Some(view_basis.clone())
            } else {
                None
            }
        });
        let basis = match direct_view_files {
            Some(b) => b,
            None => {
                let (b, _) = scoped_write_state(&session, &mut state, &[], &roots)?;
                let viewed_ok = state.view.as_ref().is_some_and(|(s, _)| s == &revision);
                if state.sync_revision != revision && !viewed_ok {
                    return Err("目录视图已更新，请重新选择文件".into());
                }
                b
            }
        };
        let mut files = BTreeMap::<String, String>::new();
        let mut directories = std::collections::BTreeSet::new();
        let mut layout = DownloadLayout::default();
        let mut empty = None;
        for path in roots {
            let root = path.rsplit('/').next().ok_or("文件名无效")?;
            let prefix = path.clone() + "/";
            if basis.resolve(&path).is_ok_and(|entry| entry.kind == "file") {
                let relative = layout.allocate(&path, "", root, false, &directory)?;
                files.insert(path.clone(), relative);
            } else {
                let relative = layout.allocate(&path, "", root, true, &directory)?;
                directories.insert(relative.clone());
                let mut count = 0;
                if basis
                    .resolve(&path)
                    .is_ok_and(|entry| entry.kind == "directory")
                {
                    empty.get_or_insert((path.clone(), relative));
                }
                for (child, _) in basis
                    .versions
                    .range(prefix.clone()..)
                    .take_while(|(child, _)| child.starts_with(&prefix))
                {
                    let entry = match basis.resolve(child) {
                        Ok(entry) => entry,
                        Err(sdk::Error::NotFound) => continue,
                        Err(e) => return Err(error(e)),
                    };
                    count += 1;
                    let mut remote = path.clone();
                    let mut local = layout.paths[&path].clone();
                    let parts = child
                        .strip_prefix(&prefix)
                        .ok_or("目录前缀无效")?
                        .split('/')
                        .collect::<Vec<_>>();
                    for (i, name) in parts.iter().enumerate() {
                        remote.push('/');
                        remote.push_str(name);
                        let is_directory = i + 1 < parts.len() || entry.kind == "directory";
                        local = layout.allocate(&remote, &local, name, is_directory, &directory)?;
                        if is_directory {
                            directories.insert(local.clone());
                        }
                    }
                    if entry.kind == "file" {
                        files.entry(child.clone()).or_insert(local);
                    } else {
                        empty.get_or_insert((child.clone(), local));
                    }
                    if files.len() + directories.len() > 10000 {
                        return Err("下载项目超过 10000 个，请分批选择".into());
                    }
                }
                if count == 0 && basis.resolve(&path).is_err() {
                    return Err("所选文件夹不存在".into());
                }
            }
            if files.len() + directories.len() > 10000 {
                return Err("下载项目超过 10000 个，请分批选择".into());
            }
        }
        if files.is_empty() {
            if let Some((path, relative)) = empty {
                files.insert(path, relative);
            }
        }
        let mut specs = vec![];
        let mut intents = vec![];
        let mut destinations = HashSet::new();
        for (path, relative) in files {
            for name in relative.split('/') {
                crate::transfers::validate_filename(name)?;
            }
            let output = directory.join(relative);
            if !destinations.insert(output.clone()) {
                return Err("所选项目的保存路径重名，请分批下载".into());
            }
            let entry = basis.resolve(&path).map_err(error)?;
            let mut intent = download_intent(
                &session,
                &basis,
                &path,
                &output,
                &directory,
                state.repo.descriptor.kind == "encrypted",
                false,
            );
            if specs.is_empty() {
                intent.directories = directories.iter().cloned().collect();
            }
            specs.push(TransferSpec {
                direction: TransferDirection::Download,
                remote: cloud_file(
                    &basis.repository,
                    path.rsplit('/').next().unwrap().into(),
                    entry.size,
                ),
                local_path: output.to_string_lossy().into_owned(),
                name: None,
            });
            intents.push(Some(intent));
        }
        Ok((specs, intents))
    })
    .await
    .map_err(|_| "下载准备已中断")??;
    state
        .transfers
        .enqueue_with_repository(specs, intents)
        .await
}

pub(super) fn download_intent(
    session: &Session,
    basis: &Snapshot,
    path: &str,
    output: &std::path::Path,
    root: &std::path::Path,
    encrypted: bool,
    preview: bool,
) -> RepositoryTransfer {
    RepositoryTransfer {
        preview,
        format: 1,
        owner: session.scope.clone(),
        folder: session.folder.clone(),
        repository: basis.repository.clone(),
        encrypted,
        basis: narrowed(basis, &[path.into()], None),
        paths: vec![path.into()],
        local_paths: vec![output.to_string_lossy().into_owned()],
        directories: vec![],
        source_root: output.to_string_lossy().into_owned(),
        plan: None,
        replace: false,
        download_root: Some(root.into()),
    }
}
type Observer = Arc<dyn Fn(&str, u64, u64) + Send + Sync>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicationJournal {
    format: u32,
    job: String,
    intent: String,
    plan: Option<sdk::repository::PreparedPublication>,
    published: Option<ObjectRef>,
}
struct Finish(Arc<AtomicBool>);
impl Drop for Finish {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release)
    }
}
struct ActiveLibrary {
    manager: Arc<RepositoryManager>,
    identity: (String, String),
}
impl Drop for ActiveLibrary {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.manager.active_transfers.lock() {
            if let Some(count) = counts.get_mut(&self.identity) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(&self.identity);
                }
            }
        }
    }
}
struct ObservedStore {
    inner: Arc<dyn ObjectStore>,
    observe: Observer,
}
impl ObjectStore for ObservedStore {
    fn progress(&self, phase: &str, done: u64, total: u64) {
        (self.observe)(phase, done, total)
    }
    fn list(&self, directory: &str) -> sdk::Result<Vec<ObjectRef>> {
        self.inner.list(directory)
    }
    fn stat(&self, reference: &ObjectRef) -> sdk::Result<ObjectRef> {
        self.inner.stat(reference)
    }
    fn open(&self, reference: &ObjectRef) -> sdk::Result<Box<dyn Read + Send>> {
        self.inner.open(reference)
    }
    fn put_immutable(
        &self,
        key: &str,
        size: u64,
        sha: &str,
        input: &mut dyn Read,
    ) -> sdk::Result<ObjectRef> {
        self.inner.put_immutable(key, size, sha, input)
    }
    fn cipher_entropy(
        &self,
        key: &str,
        sha: &str,
    ) -> sdk::Result<Option<sdk::repository::CipherEntropy>> {
        self.inner.cipher_entropy(key, sha)
    }
    fn prefetch_metadata(&self, refs: &[ObjectRef]) -> sdk::Result<()> {
        self.inner.prefetch_metadata(refs)
    }
    fn prepare(&self) -> sdk::Result<()> {
        self.inner.prepare()
    }
}
fn matching(
    manager: &RepositoryManager,
    owner: &str,
    id: &str,
    folder: &str,
) -> Result<Option<Arc<Session>>, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "对象库会话不可用")?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for session in sessions {
        if session.scope == owner
            && session.folder == folder
            && session
                .state
                .lock()
                .map_err(|_| "对象库正在操作")?
                .repo
                .descriptor
                .id
                == id
        {
            return Ok(Some(session));
        }
    }
    Ok(None)
}
fn safe_parent(root: &std::path::Path, destination: &std::path::Path) -> Result<(), String> {
    for ancestor in root.ancestors() {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            if cache::reparse(&meta) || !meta.is_dir() {
                return Err("保存目录含链接或同名文件".into());
            }
        }
    }
    std::fs::create_dir_all(root).map_err(|_| "保存目录不可用")?;
    let relative = destination
        .strip_prefix(root)
        .map_err(|_| "保存目录身份无效")?;
    let mut parent = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("保存路径无效".into());
        }
        parent.push(component.as_os_str());
        if let Ok(metadata) = std::fs::symlink_metadata(&parent) {
            if cache::reparse(&metadata) || !metadata.is_dir() {
                return Err("保存目录含链接或同名文件".into());
            }
        } else {
            std::fs::create_dir(&parent).map_err(|_| "无法创建保存目录")?;
        }
    }
    Ok(())
}
pub(crate) async fn run(
    manager: Arc<RepositoryManager>,
    app: AppHandle,
    account: AccountConfig,
    intent: RepositoryTransfer,
    job: TransferJob,
    control: Arc<Control>,
    transfers: Arc<TransferManager>,
) -> Result<(), String> {
    intent.validate()?;
    if scope(&account) != intent.owner {
        return Err("请切回添加任务时的云盘账号".into());
    }
    // One encrypted payload owns the shared spool. Waiting is cancellable and
    // does not consume upload attempts or misreport a cache lock as a bad key.
    let _slot = if intent.encrypted {
        loop {
            control.check()?;
            tokio::select! {permit=manager.payload_slot.acquire()=>break Some(permit.map_err(|_|"加密传输正在停止")?),_=tokio::time::sleep(Duration::from_millis(100))=>{}}
        }
    } else {
        None
    };
    let identity = (intent.owner.clone(), intent.repository.clone());
    *manager
        .active_transfers
        .lock()
        .map_err(|_| "对象库传输状态不可用")?
        .entry(identity.clone())
        .or_default() += 1;
    let _active = ActiveLibrary {
        manager: manager.clone(),
        identity,
    };
    let mut opened = None;
    let session = if let Some(session) =
        matching(&manager, &intent.owner, &intent.repository, &intent.folder)?
    {
        session
    } else {
        let unlock = if intent.encrypted {
            let store = app.state::<AppState>();
            let raw = system_secrets::load(
                &store.store,
                Some(&app),
                &secret_id(&intent.owner, &intent.repository),
            )
            .await?
            .ok_or("请先重新打开并解锁该对象库，再开始任务")?;
            serde_json::from_slice::<Unlock>(&raw).map_err(|_| "本机对象库凭据格式无效")?
        } else {
            Unlock {
                password: String::new(),
                key: String::new(),
                key_file: None,
            }
        };
        let m = manager.clone();
        let cfg = account.clone();
        let folder = intent.folder.clone();
        let id = intent.repository.clone();
        let (handle, _) = tokio::task::spawn_blocking(move || m.open(&cfg, &folder, &id, &unlock))
            .await
            .map_err(|_| "对象库解锁已中断")??;
        let session = manager.session(&handle, &intent.owner)?;
        opened = Some(handle);
        session
    };
    let m = manager.clone();
    let a = app.clone();
    let out = tokio::task::spawn_blocking(move || -> Result<(), String> {
        control.check()?;
        let backend = m.backend(&account, &intent.folder, true)?;
        control.attach_cancel(backend.cancel_flag())?;
        let confirmed = Arc::new(std::sync::atomic::AtomicU64::new(0));
        backend.observe_upload(confirmed.clone()).map_err(error)?;
        let meter = Arc::new(Mutex::new((
            String::new(),
            0u64,
            Instant::now(),
            Instant::now(),
        )));
        let stage = Arc::new(Mutex::new((String::new(), 0u64)));
        let phase = stage.clone();
        let progress_control = control.clone();
        let progress_transfers = transfers.clone();
        let progress_job = job.clone();
        let observer: Observer = Arc::new(move |name, done, total| {
            if let Ok(mut value) = phase.lock() {
                *value = (name.into(), total)
            }
            if let Ok(mut last) = meter.lock() {
                let changed = last.0 != name;
                if changed || last.3.elapsed() >= Duration::from_millis(150) || done >= total {
                    let speed =
                        if changed || matches!(name, "hashing" | "encrypting" | "decrypting") {
                            0.0
                        } else {
                            done.saturating_sub(last.1) as f64
                                / last.2.elapsed().as_secs_f64().max(0.001)
                        };
                    progress_transfers.progress(&progress_job, done, total, speed, name);
                    *last = (name.into(), done, Instant::now(), Instant::now());
                }
            }
            let _ = progress_control.check();
        });
        let cached = m.cached_backend(&account, &intent.folder, backend.clone())?;
        let store: Arc<dyn ObjectStore> = Arc::new(ObservedStore {
            inner: cached,
            observe: observer.clone(),
        });
        let mut repo = session
            .state
            .lock()
            .map_err(|_| "对象库正在操作")?
            .repo
            .fork_for_maintenance_using(store)
            .map_err(error)?;
        let scratch = sdk::repository::Scratch {
            directory: m.cache.join("repository-transfers"),
            max_bytes: 2 << 30,
            min_free_bytes: 2 << 30,
        };
        let finished = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let done = finished.clone();
            let progress = observer.clone();
            let phase = stage.clone();
            let bytes = confirmed.clone();
            scope.spawn(move || {
                while !done.load(Ordering::Acquire) {
                    if let Ok((name, total)) = phase.lock().map(|v| v.clone()) {
                        if name == "uploading" {
                            progress("uploading", bytes.load(Ordering::Acquire).min(total), total)
                        }
                    }
                    std::thread::sleep(Duration::from_millis(150));
                }
            });
            let finish = Finish(finished.clone());
            let result = (|| -> Result<(), String> {
                if let Some(plan) = &intent.plan {
                    let intent_hash =
                        digest(&serde_json::to_vec(&intent).map_err(|_| "传输身份编码失败")?);
                    let mut journal = if let Some(raw) = transfers.journal(&job.id)? {
                        let saved: PublicationJournal =
                            serde_json::from_str(&raw).map_err(|_| "原目录提交断点已损坏")?;
                        if saved.format != 1 || saved.job != job.id || saved.intent != intent_hash {
                            return Err("原目录提交身份不一致".into());
                        }
                        saved
                    } else {
                        let mut sources = vec![];
                        for ((path, local), item) in intent
                            .paths
                            .iter()
                            .zip(&intent.local_paths)
                            .zip(&plan.items)
                        {
                            let app = a.clone();
                            let local = local.clone();
                            let cancel = backend.cancel_flag();
                            let size = item.size;
                            sources.push(UploadSource {
                                path: path.clone(),
                                size,
                                modified_at: item.modified_at,
                                open: Arc::new(move || {
                                    let p = FilePath::from_str(&local)
                                        .map_err(|_| sdk::Error::Invalid)?;
                                    let file = app
                                        .fs()
                                        .open(p, OpenOptions::new().read(true).clone())
                                        .map_err(|_| sdk::Error::Incomplete)?;
                                    Ok(Box::new(CancelReader {
                                        inner: file,
                                        cancel: cancel.clone(),
                                    }))
                                }),
                            });
                        }
                        observer("hashing", 0, plan.source_bytes);
                        let prepared = repo
                            .prepare_upload_tree(
                                &intent.basis,
                                &digest(intent.owner.as_bytes())[..32],
                                &sources,
                                plan,
                                Some(&scratch),
                                intent.replace,
                                &intent.directories,
                            )
                            .map_err(error)?;
                        let saved = PublicationJournal {
                            format: 1,
                            job: job.id.clone(),
                            intent: intent_hash,
                            plan: prepared,
                            published: None,
                        };
                        transfers.save_journal(&job.id, &saved)?;
                        saved
                    };
                    if let Some(plan) = &journal.plan {
                        observer("publishing", 0, job.total_bytes);
                        journal.published = Some(repo.publish_prepared(plan).map_err(error)?);
                        transfers.save_journal(&job.id, &journal)?;
                    }
                    repo.save_observed_file(&session.observation)
                        .map_err(error)?;
                    let _ = a.emit(
                        "repository-directory-changed",
                        serde_json::json!({"repositoryId":intent.repository}),
                    );
                } else {
                    let root = intent.download_root.as_ref().ok_or("保存目录不可用")?;
                    for directory in &intent.directories {
                        control.check()?;
                        safe_parent(root, &root.join(directory).join(".directory"))?;
                    }
                    let destination = PathBuf::from(&job.local_path);
                    safe_parent(root, &destination)?;
                    if intent.basis.resolve(&intent.paths[0]).map_err(error)?.kind == "directory" {
                        safe_parent(root, &destination.join(".directory"))?;
                    } else {
                        repo.export_file_staged(
                            &intent.basis,
                            &intent.paths[0],
                            &destination,
                            &scratch,
                            &|_| control.check().map_err(|_| sdk::Error::Incomplete),
                        )
                        .map_err(error)?;
                    }
                }
                control.check()?;
                transfers.checkpoint(&job.id, job.total_bytes, job.total_bytes)?;
                observer("completed", job.total_bytes, job.total_bytes);
                Ok(())
            })();
            drop(finish);
            result
        })
    })
    .await
    .map_err(|_| "对象库传输已中断，检查点已保留")?;
    if let Some(handle) = opened {
        manager.close(&handle)
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_layout_keeps_hierarchy_and_avoids_case_and_existing_file_collisions() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("existing.txt"), b"keep").unwrap();
        let mut layout = DownloadLayout::default();
        let folder = layout
            .allocate("remote/A", "", "A", true, root.path())
            .unwrap();
        assert_eq!(folder, "A");
        assert_eq!(
            layout
                .allocate("remote/A/file", "A", "file.txt", false, root.path())
                .unwrap(),
            "A/file.txt"
        );
        assert_eq!(
            layout
                .allocate("remote/A/other", "A", "FILE.txt", false, root.path())
                .unwrap(),
            "A/FILE (1).txt"
        );
        assert_eq!(
            layout
                .allocate("remote/existing", "", "existing.txt", false, root.path())
                .unwrap(),
            "existing (1).txt"
        );
        assert_eq!(
            std::fs::read(root.path().join("existing.txt")).unwrap(),
            b"keep"
        );
    }
    #[test]
    fn empty_download_directories_are_created_without_allowing_parent_escape() {
        let root = tempfile::tempdir().unwrap();
        safe_parent(root.path(), &root.path().join("tree/empty/.directory")).unwrap();
        assert!(root.path().join("tree/empty").is_dir());
        assert!(safe_parent(root.path(), &root.path().join("../outside/file")).is_err());
        let bad = RepositoryTransfer {
            preview: false,
            format: 1,
            owner: "a".repeat(64),
            folder: "folder".into(),
            repository: "b".repeat(32),
            encrypted: false,
            basis: Snapshot {
                repository: "b".repeat(32),
                heads: vec![],
                versions: BTreeMap::new(),
                conflicts: vec![],
            },
            paths: vec!["tree".into()],
            local_paths: vec![root.path().join("tree").to_string_lossy().into_owned()],
            directories: vec!["../outside".into()],
            source_root: root.path().join("tree").to_string_lossy().into_owned(),
            plan: None,
            replace: false,
            download_root: Some(root.path().into()),
        };
        assert!(bad.validate().is_err());
    }
}
