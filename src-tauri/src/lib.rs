mod auth;
mod archive_format;
mod file_preview;
mod batch_control;
mod cloud;
mod official;
mod openlist_crypt;
mod openlist_crypt_cipher;
mod openlist_crypt_media;
mod repositories;
mod repository_index;
mod sessions;
mod store;
mod system_secrets;
mod transfer_activity;
mod transfers;
#[cfg(desktop)]
mod tray;
#[cfg(desktop)]
mod upload_paths;

use cloud::{AccountConfig, CloudClient, CloudFile, FilePage, Quota};
use sessions::BrowseSource;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use store::Store;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_document_picker::DocumentPickerExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex, RwLock};
use transfers::{TransferDirection, TransferJob, TransferManager, TransferSpec};

struct AppState {
    client: RwLock<Option<CloudClient>>,
    pending_login: RwLock<Option<AccountConfig>>,
    session: Mutex<()>,
    scan: Mutex<()>,
    scan_cancel: Arc<AtomicBool>,
    #[cfg(desktop)]
    tray_ready: AtomicBool,
    store: Arc<Store>,
    transfers: Arc<TransferManager>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSummary {
    id: String,
    name: String,
    account: String,
    phone: String,
    space: String,
    root_id: String,
    cloud_id: String,
    user_domain_id: String,
    connected: bool,
}
fn summary(config: &AccountConfig) -> AccountSummary {
    let account = auth::account_from_authorization(&config.authorization)
        .unwrap_or_else(|_| config.username.clone());
    let chars: Vec<char> = account.chars().collect();
    let masked = if chars.len() >= 7 {
        format!(
            "{}****{}",
            chars[..3].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    } else {
        "分享访客".into()
    };
    AccountSummary {
        id: config.id.clone(),
        name: config.name.clone(),
        account: masked,
        phone: if account.len() == 11 && account.bytes().all(|c| c.is_ascii_digit()) {
            account
        } else {
            String::new()
        },
        space: config.space.clone(),
        root_id: config.root_id.clone(),
        cloud_id: config.cloud_id.clone(),
        user_domain_id: config.user_domain_id.clone(),
        connected: true,
    }
}
fn persist(store: &Store, config: &AccountConfig) -> Result<(), String> {
    let mut vault = sessions::Sessions::load(store)?;
    vault.upsert(config.clone())?;
    vault.save(store)
}
async fn client(state: &AppState) -> Result<CloudClient, String> {
    state
        .client
        .read()
        .await
        .clone()
        .ok_or_else(|| "请先连接移动云盘".into())
}
async fn file_client(state: &AppState, file: &CloudFile) -> Result<CloudClient, String> {
    if let Some(id) = &file.source_id {
        let account = client(state).await?;
        let owner = account.account_config().await.id;
        let vault = sessions::Sessions::load(&state.store)?;
        return vault.source(&owner, id, false)?.client(&account).await;
    }
    client(state).await
}
fn require_writable(file: &CloudFile) -> Result<(), String> {
    if file.source_id.is_some() {
        return Err("群组与分享仅支持只读浏览".into());
    }
    Ok(())
}
async fn connect(state: &AppState, config: AccountConfig) -> Result<AccountSummary, String> {
    state.scan_cancel.store(true, Ordering::Release);
    let _guard = state.session.lock().await;
    connect_locked(state, config).await
}
async fn connect_locked(
    state: &AppState,
    config: AccountConfig,
) -> Result<AccountSummary, String> {
    activate_account(state, config, None).await
}
async fn activate_account(
    state: &AppState,
    mut config: AccountConfig,
    remove: Option<&str>,
) -> Result<AccountSummary, String> {
    state.scan_cancel.store(true, Ordering::Release);
    config.keep_personal_space();
    let mut vault = sessions::Sessions::load(&state.store)?;
    let adding = config.id.is_empty();
    if config.id.is_empty() {
        config.id = vault
            .matching_id(&config)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    }
    if !config.sms_code.is_empty() && config.mail_cookies.is_empty() {
        if let Some(pending) = state.pending_login.read().await.as_ref() {
            if pending.username == config.username {
                config.mail_cookies = pending.mail_cookies.clone();
            }
        }
    }
    let candidate = CloudClient::new(config)?;
    let mut updated = match candidate.initialize().await {
        Ok(updated) => updated,
        Err(error) => {
            let mut pending = candidate.account_config().await;
            let keep = error.starts_with("SMS_REQUIRED")
                || state
                    .pending_login
                    .read()
                    .await
                    .as_ref()
                    .is_some_and(|previous| previous.username == pending.username);
            if keep {
                pending.password.clear();
                pending.sms_code.clear();
                *state.pending_login.write().await = Some(pending);
            }
            return Err(error);
        }
    };
    if adding {
        if let Some(id) = vault.matching_id(&updated) {
            updated.id = id.clone();
            candidate.set_account_id(id).await;
        }
    }
    if let Some(id) = remove {
        vault.accounts.retain(|config| config.id != id);
    }
    vault.upsert(updated.clone())?;
    vault.save(&state.store)?;
    *state.pending_login.write().await = None;
    let result = summary(&updated);
    state.transfers.set_client(Some(candidate.clone())).await;
    *state.client.write().await = Some(candidate);
    Ok(result)
}
#[tauri::command]
async fn connect_account(
    state: State<'_, AppState>,
    config: AccountConfig,
) -> Result<AccountSummary, String> {
    connect(&state, config).await
}
#[tauri::command]
async fn get_account(state: State<'_, AppState>) -> Result<Option<AccountSummary>, String> {
    let _guard = state.session.lock().await;
    if let Some(current) = state.client.read().await.clone() {
        return Ok(Some(summary(&current.account_config().await)));
    }
    let vault = sessions::Sessions::load(&state.store)?;
    let mut accounts = vault.accounts;
    accounts.sort_by_key(|config| Some(&config.id) != vault.active_id.as_ref());
    let mut first_error = None;
    for config in accounts {
        match connect_locked(&state, config).await {
            Ok(account) => return Ok(Some(account)),
            Err(error) => { first_error.get_or_insert(error); }
        }
    }
    first_error.map_or(Ok(None), Err)
}
#[tauri::command]
async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountSummary>, String> {
    let _guard = state.session.lock().await;
    let active = state
        .client
        .read()
        .await
        .as_ref()
        .map(|client| client.clone());
    let active_id = match active {
        Some(client) => Some(client.account_config().await.id),
        None => None,
    };
    Ok(sessions::Sessions::load(&state.store)?
        .accounts
        .iter()
        .map(|config| {
            let mut result = summary(config);
            result.connected = Some(&config.id) == active_id.as_ref();
            result
        })
        .collect())
}
#[tauri::command]
async fn select_account(state: State<'_, AppState>, id: String) -> Result<AccountSummary, String> {
    state.scan_cancel.store(true, Ordering::Release);
    let _guard = state.session.lock().await;
    let config = sessions::Sessions::load(&state.store)?
        .accounts
        .into_iter()
        .find(|config| config.id == id)
        .ok_or("该账号已删除，请重新登录")?;
    connect_locked(&state, config).await
}
#[tauri::command]
async fn remove_account(state: State<'_, AppState>, id: String) -> Result<Option<AccountSummary>, String> {
    let _guard = state.session.lock().await;
    let mut vault = sessions::Sessions::load(&state.store)?;
    if !vault.accounts.iter().any(|config| config.id == id) {
        return Ok(None);
    }
    if vault.accounts.len() == 1 {
        return Err("请先添加另一个账号，再删除此账号".into());
    }
    if vault.active_id.as_deref() == Some(&id) {
        let mut first_error = None;
        for replacement in vault.accounts.into_iter().filter(|entry| entry.id != id) {
            match activate_account(&state, replacement, Some(&id)).await {
                Ok(account) => return Ok(Some(account)),
                Err(error) => { first_error.get_or_insert(error); }
            }
        }
        return Err(format!("无法切换到其他账号：{}", first_error.unwrap_or_default()));
    }
    vault.accounts.retain(|config| config.id != id);
    vault.save(&state.store)?;
    Ok(None)
}
#[tauri::command]
async fn discover_official() -> Result<official::OfficialDiscovery, String> {
    tokio::task::spawn_blocking(official::discover_official)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn import_official(state: State<'_, AppState>) -> Result<AccountSummary, String> {
    let credentials = tokio::task::spawn_blocking(official::import_official_credentials)
        .await
        .map_err(|e| e.to_string())??;
    let config = AccountConfig {
        authorization: credentials.authorization,
        username: credentials.username.unwrap_or_default(),
        mail_cookies: credentials.mail_cookies.unwrap_or_default(),
        user_domain_id: credentials.user_domain_id.unwrap_or_default(),
        ..Default::default()
    };
    connect(&state, config).await
}
#[tauri::command]
async fn list_browse_sources(state: State<'_, AppState>, account_id: String) -> Result<Vec<BrowseSource>, String> {
    let _guard = state.session.lock().await;
    browse_account(&state, &account_id).await?;
    Ok(sessions::Sessions::load(&state.store)?.sources.into_iter()
        .filter(|source| source.owner_id == account_id && !source.removed)
        .map(browse_source_summary)
        .collect())
}
async fn browse_account(state: &AppState, expected_id: &str) -> Result<CloudClient, String> {
    let account = client(state).await?;
    if account.account_config().await.id != expected_id {
        return Err("当前账号已切换，请重试".into());
    }
    Ok(account)
}
fn browse_source_summary(mut source: BrowseSource) -> BrowseSource {
    source.reference.clear();
    source.owner_id.clear();
    source
}
#[tauri::command]
async fn add_browse_source(
    state: State<'_, AppState>,
    account_id: String,
    kind: String,
    input: String,
) -> Result<BrowseSource, String> {
    let reference = match kind.as_str() {
        "share" => cloud::normalize_share_input(&input)?,
        "group" => {
            let input = input.trim();
            if input.is_empty() || input.len() > 128 || !input.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
                return Err("请填写有效的群组 ID".into());
            }
            input.into()
        }
        _ => return Err("未知的浏览来源".into()),
    };
    let _guard = state.session.lock().await;
    let account = browse_account(&state, &account_id).await?;
    let mut vault = sessions::Sessions::load(&state.store)?;
    let existing = vault.sources.iter().position(|source| {
        source.owner_id == account_id && source.kind == kind
            && source.reference.split('#').next() == reference.split('#').next()
    });
    let mut source = BrowseSource {
        id: existing.map(|i| vault.sources[i].id.clone()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        kind,
        name: String::new(),
        owner_id: account_id,
        removed: false,
        reference,
    };
    let source_client = source.client(&account).await?;
    source.name = if source.kind == "share" {
        source_client.share_title().await?
    } else {
        source_client.list_page(&source_client.root().await, None).await?;
        format!("群组 {}", source.reference)
    };
    if let Some(index) = existing {
        vault.sources[index] = source.clone();
    } else {
        vault.sources.push(source.clone());
    }
    vault.save(&state.store)?;
    Ok(browse_source_summary(source))
}
#[tauri::command]
async fn remove_browse_source(state: State<'_, AppState>, account_id: String, id: String) -> Result<(), String> {
    let _guard = state.session.lock().await;
    browse_account(&state, &account_id).await?;
    let mut vault = sessions::Sessions::load(&state.store)?;
    if let Some(source) = vault.sources.iter_mut().find(|source| {
        source.owner_id == account_id && source.id == id && !source.removed
    }) {
        // Queued downloads still need the source credentials after its list entry is removed.
        source.removed = true;
        vault.save(&state.store)?;
    }
    Ok(())
}
#[tauri::command]
async fn update_options(
    state: State<'_, AppState>,
    config: Value,
) -> Result<AccountSummary, String> {
    let mut updated = client(&state).await?.account_config().await;
    if let Some(value) = config["customUploadPartSize"].as_u64() {
        updated.custom_upload_part_size = value;
    }
    if let Some(value) = config["useLargeThumbnail"].as_bool() {
        updated.use_large_thumbnail = value;
    }
    connect(&state, updated).await
}
#[tauri::command]
async fn get_options(state: State<'_, AppState>) -> Result<Value, String> {
    let config = match state.client.read().await.as_ref() {
        Some(client) => client.account_config().await,
        None => AccountConfig::default(),
    };
    Ok(
        serde_json::json!({"concurrency":state.transfers.concurrency(),"customUploadPartSize":config.custom_upload_part_size,"useLargeThumbnail":config.use_large_thumbnail}),
    )
}
#[tauri::command]
async fn get_foreground_status(
    app: tauri::AppHandle,
) -> Result<tauri_plugin_document_picker::ForegroundStatus, String> {
    app.document_picker().foreground_status().await
}
#[tauri::command]
async fn open_completed_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    reveal: bool,
) -> Result<(), String> {
    use rusqlite::OptionalExtension;
    let path: Option<String> = {
        let db = state
            .store
            .db
            .lock()
            .map_err(|_| "传输记录不可用".to_string())?;
        db.query_row("SELECT COALESCE(shared_download_uri,local_path) FROM transfers WHERE id=?1 AND direction='download' AND status='completed'",[id],|row|row.get(0)).optional().map_err(|e|e.to_string())?
    };
    let path = path.ok_or("仅支持打开已完成的下载")?;
    if !path.starts_with("content://") && !path.starts_with("file://") && !tokio::fs::try_exists(&path)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err("本地文件已被移动或删除".into());
    }
    #[cfg(desktop)]
    {
        tokio::task::spawn_blocking(move || {
            if reveal {
                app.opener().reveal_item_in_dir(&path)
            } else {
                app.opener().open_path(path, None::<&str>)
            }
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        let _ = reveal;
        app.document_picker().open_download(&path).await
    }
    #[cfg(target_os = "ios")]
    {
        let _ = reveal;
        app.opener()
            .open_url(
                url::Url::from_file_path(path)
                    .map_err(|_| "文件路径无效")?
                    .to_string(),
                None::<&str>,
            )
            .map_err(|e| e.to_string())
    }
}
#[tauri::command]
async fn export_completed_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    use rusqlite::OptionalExtension;
    let path: Option<String> = {
        let db = state
            .store
            .db
            .lock()
            .map_err(|_| "传输记录不可用".to_string())?;
        db.query_row("SELECT local_path FROM transfers WHERE id=?1 AND direction='download' AND status='completed'",[id],|row|row.get(0)).optional().map_err(|e|e.to_string())?
    };
    let path = path.ok_or("仅支持导出已完成的下载")?;
    app.document_picker().export_download(&path).await
}
#[tauri::command]
async fn list_files(
    state: State<'_, AppState>,
    parent: CloudFile,
    cursor: Option<String>,
) -> Result<FilePage, String> {
    let current = file_client(&state, &parent).await?;
    let mut directory = parent;
    if directory.source_id.is_some() && directory.id.is_empty() {
        directory.id = current.root().await.id;
    }
    let mut page = current.list_page(&directory, cursor).await?;
    for file in &mut page.files {
        file.source_id.clone_from(&directory.source_id);
    }
    Ok(page)
}
#[tauri::command]
async fn get_link(state: State<'_, AppState>, file: CloudFile) -> Result<String, String> {
    file_client(&state, &file).await?.link(&file).await
}
#[tauri::command]
async fn get_preview(state: State<'_, AppState>, file: CloudFile) -> Result<Value, String> {
    file_client(&state, &file).await?.preview(&file).await
}
#[tauri::command]
async fn get_quota(state: State<'_, AppState>) -> Result<Quota, String> {
    client(&state).await?.quota().await
}
#[tauri::command]
async fn make_directory(
    state: State<'_, AppState>,
    parent: CloudFile,
    name: String,
) -> Result<(), String> {
    require_writable(&parent)?;
    validate_name(&name)?;
    client(&state).await?.mkdir(&parent, &name).await
}
#[tauri::command]
async fn rename_file(
    state: State<'_, AppState>,
    file: CloudFile,
    name: String,
) -> Result<(), String> {
    require_writable(&file)?;
    validate_name(&name)?;
    client(&state).await?.rename(&file, &name).await
}
fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        Err("文件名称无效".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
async fn remove_files(state: State<'_, AppState>, files: Vec<CloudFile>) -> Result<Value, String> {
    if files.len() > 10000 {
        return Err("每批操作最多 10,000 个项目".into());
    }
    files.iter().try_for_each(require_writable)?;
    let current = client(&state).await?;
    let mut failures = Vec::new();
    let mut removed_ids = Vec::new();
    for file in files {
        if let Err(error) = current.remove(&file).await {
            failures.push(format!("{}：{error}", file.name));
            if failures.len() >= 10 {
                failures.push("达到错误上限，剩余项目未处理；请检查后重试".into());
                break;
            }
        } else {
            removed_ids.push(file.id);
        }
    }
    Ok(serde_json::json!({"removedIds": removed_ids, "errors": failures}))
}
#[tauri::command]
async fn relocate_files(
    state: State<'_, AppState>,
    files: Vec<CloudFile>,
    destination: CloudFile,
    copy: bool,
) -> Result<(), String> {
    if files.len() > 10000 {
        return Err("每批操作最多 10,000 个项目".into());
    }
    require_writable(&destination)?;
    files.iter().try_for_each(require_writable)?;
    let current = client(&state).await?;
    for file in files {
        if file.id == destination.id
            || file.is_dir
                && destination
                    .path
                    .starts_with(&(file.path.trim_end_matches('/').to_owned() + "/"))
        {
            return Err("不能将文件夹移动或复制到自身内部".into());
        }
        if copy {
            current.copy(&file, &destination).await?;
        } else {
            current.move_file(&file, &destination).await?;
        }
    }
    Ok(())
}
#[tauri::command]
async fn queue_transfers(
    state: State<'_, AppState>,
    specs: Vec<TransferSpec>,
) -> Result<Vec<TransferJob>, String> {
    for spec in &specs {
        if spec.direction == TransferDirection::Upload {
            require_writable(&spec.remote)?;
        }
    }
    let _session = state.session.lock().await;
    state.transfers.enqueue(specs).await
}
#[tauri::command]
async fn queue_uploads(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    parent: CloudFile,
) -> Result<Vec<TransferJob>, String> {
    require_writable(&parent)?;
    let _session = state.session.lock().await;
    #[cfg(target_os = "android")]
    {
        let documents = app.document_picker().pick_files().await?;
        let specs = documents
            .into_iter()
            .map(|document| TransferSpec {
                direction: TransferDirection::Upload,
                remote: parent.clone(),
                local_path: document.path,
                name: Some(document.name),
            })
            .collect();
        return state.transfers.enqueue(specs).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let paths = tokio::task::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_title("选择上传文件")
                .blocking_pick_files()
        })
        .await
        .map_err(|e| e.to_string())?;
        let Some(paths) = paths else {
            return Ok(Vec::new());
        };
        let specs = paths
            .into_iter()
            .map(|path| TransferSpec {
                direction: TransferDirection::Upload,
                remote: parent.clone(),
                local_path: path.to_string(),
                name: None,
            })
            .collect();
        state.transfers.enqueue(specs).await
    }
}
#[tauri::command]
async fn queue_downloads(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    files: Vec<CloudFile>,
) -> Result<Vec<TransferJob>, String> {
    let _session = state.session.lock().await;
    let _scan = state
        .scan
        .try_lock()
        .map_err(|_| "已有目录扫描正在进行，请完成或取消后再添加".to_string())?;
    state.scan_cancel.store(false, Ordering::Release);
    client(&state).await?;
    let Some(first) = files.first() else { return Ok(Vec::new()); };
    if files.iter().any(|file| file.source_id != first.source_id) {
        return Err("请从同一个来源选择下载项目".into());
    }
    let current = file_client(&state, first).await?;
    #[cfg(desktop)]
    let directory = {
        let picker = app.clone();
        let picked = tokio::task::spawn_blocking(move || {
            picker
                .dialog()
                .file()
                .set_title("选择下载文件夹")
                .blocking_pick_folder()
        })
        .await
        .map_err(|e| e.to_string())?;
        let Some(picked) = picked else {
            return Ok(Vec::new());
        };
        picked.into_path().map_err(|e| e.to_string())?
    };
    #[cfg(mobile)]
    let directory = {
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("Downloads");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        directory
    };
    enqueue_download_tree(&app, &state, current, files, directory).await
}
async fn enqueue_download_tree(
    app: &tauri::AppHandle,
    state: &AppState,
    current: CloudClient,
    files: Vec<CloudFile>,
    directory: PathBuf,
) -> Result<Vec<TransferJob>, String> {
    let mut pending: VecDeque<_> = files
        .into_iter()
        .map(|file| (file, directory.clone()))
        .collect();
    let mut visited = HashSet::new();
    let mut batch = Vec::with_capacity(100);
    let mut result = Vec::new();
    let mut total = 0usize;
    let mut scanned = 0usize;
    let mut paths: HashMap<PathBuf, String> = HashMap::new();
    while let Some((mut file, path)) = pending.pop_front() {
        if state.scan_cancel.load(Ordering::Acquire) {
            return Err("扫描已取消；已加入的任务保留在传输列表".into());
        }
        if !visited.insert(file.id.clone()) {
            continue;
        }
        scanned += 1;
        if scanned > 100_000 || scanned + pending.len() > 100_000 {
            return Err("目录过大：本次下载最多 100,000 项，已加入的任务可在传输列表查看".into());
        }
        let name = transfers::sanitize_filename(&file.name);
        let mut target = path.join(&name);
        let mut collision = 0usize;
        loop {
            #[cfg(windows)]
            let path_key = PathBuf::from(target.to_string_lossy().to_lowercase());
            #[cfg(not(windows))]
            let path_key = target.clone();
            if !paths.contains_key(&path_key) {
                paths.insert(path_key, file.id.clone());
                break;
            }
            collision += 1;
            let filename = PathBuf::from(&name);
            let stem = filename
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("文件");
            target = path.join(
                if let Some(ext) = filename.extension().and_then(|x| x.to_str()) {
                    format!("{stem} ({collision}).{ext}")
                } else {
                    format!("{name} ({collision})")
                },
            );
        }
        if file.is_dir {
            if file.source_id.is_some() && file.id.is_empty() {
                file.id = current.root().await.id;
            }
            tokio::fs::create_dir_all(&target)
                .await
                .map_err(|e| e.to_string())?;
            let mut cursor = None;
            let mut seen = HashSet::new();
            loop {
                if state.scan_cancel.load(Ordering::Acquire) {
                    return Err("扫描已取消；已加入任务保留在传输列表".into());
                }
                let page = current.list_page(&file, cursor.clone()).await?;
                if scanned + pending.len() + page.files.len() > 100000 {
                    return Err("目录超过 100,000 项；已加入任务可在传输列表查看".into());
                }
                pending.extend(page.files.into_iter().map(|mut child| {
                    child.source_id.clone_from(&file.source_id);
                    (child, target.clone())
                }));
                match page.next_cursor {
                    Some(next) if seen.insert(next.clone()) => cursor = Some(next),
                    Some(_) => return Err("云端目录分页没有前进".into()),
                    None => break,
                }
            }
        } else {
            batch.push(TransferSpec {
                direction: TransferDirection::Download,
                remote: file,
                local_path: target.to_string_lossy().into_owned(),
                name: None,
            });
            total += 1;
        }
        if batch.len() >= 100 {
            let jobs = state.transfers.enqueue(std::mem::take(&mut batch)).await?;
            result.extend(jobs.into_iter().take(200usize.saturating_sub(result.len())));
            let _ = app.emit(
                "scan-progress",
                serde_json::json!({"queued":total,"path":path.to_string_lossy()}),
            );
        }
    }
    if !batch.is_empty() {
        let jobs = state.transfers.enqueue(batch).await?;
        result.extend(jobs.into_iter().take(200usize.saturating_sub(result.len())));
    }
    Ok(result)
}
#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) {
    state.scan_cancel.store(true, Ordering::Release);
}
async fn ensure_upload_folder(
    current: &CloudClient,
    parent: &CloudFile,
    name: &str,
    cache: &mut HashMap<String, HashMap<String, CloudFile>>,
) -> Result<CloudFile, String> {
    validate_name(name)?;
    if !cache.contains_key(&parent.id) {
        // Traversal processes one parent at a time. Keep only that directory's
        // metadata, so many large destination directories cannot multiply RAM.
        cache.clear();
        let mut entries = HashMap::new();
        let mut cursor = None;
        let mut seen = HashSet::new();
        loop {
            let page = current.list_page(parent, cursor.clone()).await?;
            for file in page.files {
                entries.insert(file.name.clone(), file);
            }
            if entries.len() > 100000 {
                return Err("单个目录超过 100,000 项，请拆分批次".into());
            }
            match page.next_cursor {
                Some(next) if seen.insert(next.clone()) => cursor = Some(next),
                Some(_) => return Err("云端分页没有前进".into()),
                None => break,
            }
        }
        cache.insert(parent.id.clone(), entries);
    }
    if let Some(file) = cache.get(&parent.id).and_then(|entries| entries.get(name)) {
        if file.is_dir {
            return Ok(file.clone());
        }
        return Err(format!("云端已有同名文件：{name}"));
    }
    let folder = current.create_directory(parent, name).await?;
    cache
        .entry(parent.id.clone())
        .or_default()
        .insert(name.to_owned(), folder.clone());
    Ok(folder)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadScanResult {
    pub(crate) queued: usize,
    pub(crate) skipped: usize,
}
#[tauri::command]
async fn queue_upload_directory(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    parent: CloudFile,
    paths: Option<Vec<String>>,
) -> Result<UploadScanResult, String> {
    require_writable(&parent)?;
    #[cfg(not(desktop))]
    if paths.is_some() {
        return Err("当前平台不支持本机路径拖拽上传".into());
    }
    let _session = state.session.lock().await;
    let _scan = state
        .scan
        .try_lock()
        .map_err(|_| "已有目录扫描正在进行，请完成或取消后再添加".to_string())?;
    state.scan_cancel.store(false, Ordering::Release);
    let current = client(&state).await?;
    let config = current.account_config().await;
    if config.space == "share" {
        return Err("分享空间不支持上传".into());
    }
    let mut batch = Vec::with_capacity(100);
    let mut queued = 0usize;
    let mut skipped = 0usize;
    let mut scanned = 0usize;
    let mut folder_cache = HashMap::new();
    #[cfg(target_os = "android")]
    {
        let Some(directory) = app.document_picker().pick_directory().await? else {
            return Ok(UploadScanResult {
                queued: 0,
                skipped: 0,
            });
        };
        let remote =
            ensure_upload_folder(&current, &parent, &directory.name, &mut folder_cache).await?;
        let mut pending = VecDeque::from([(directory.path.clone(), remote)]);
        let mut visited = HashSet::new();
        while let Some((local, remote)) = pending.pop_front() {
            if !visited.insert(local.clone()) {
                continue;
            }
            let mut cursor = None;
            loop {
                if state.scan_cancel.load(Ordering::Acquire) {
                    if let Some(cursor) = cursor.as_deref() {
                        let _ = app.document_picker().close_scan(cursor).await;
                    }
                    return Err("扫描已取消；已加入任务保留在传输列表".into());
                }
                let page = app
                    .document_picker()
                    .list_children(&directory.path, Some(&local), cursor.as_deref())
                    .await?;
                for entry in page.entries {
                    scanned += 1;
                    if scanned > 100000 {
                        return Err("目录超过 100,000 项；已加入任务可在传输列表查看".into());
                    }
                    if state.scan_cancel.load(Ordering::Acquire) {
                        if let Some(cursor) = page.next_cursor.as_deref() {
                            let _ = app.document_picker().close_scan(cursor).await;
                        }
                        return Err("扫描已取消；已加入任务保留在传输列表".into());
                    }
                    if entry.is_directory {
                        let folder =
                            ensure_upload_folder(&current, &remote, &entry.name, &mut folder_cache)
                                .await?;
                        pending.push_back((entry.path, folder));
                    } else {
                        batch.push(TransferSpec {
                            direction: TransferDirection::Upload,
                            remote: remote.clone(),
                            local_path: entry.path,
                            name: Some(entry.name),
                        });
                    }
                    if batch.len() >= 100 {
                        let jobs = state.transfers.enqueue(std::mem::take(&mut batch)).await?;
                        queued += jobs.len();
                        let _ = app.emit(
                            "scan-progress",
                            serde_json::json!({"queued":queued,"path":remote.name}),
                        );
                    }
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
        }
    }
    #[cfg(desktop)]
    {
        let supplied = paths.is_some();
        let paths = if let Some(paths) = paths {
            paths
        } else {
            let picker = app.clone();
            let picked = tokio::task::spawn_blocking(move || {
                picker
                    .dialog()
                    .file()
                    .set_title("选择上传文件夹")
                    .blocking_pick_folder()
            })
            .await
            .map_err(|e| e.to_string())?;
            let Some(picked) = picked else {
                return Ok(UploadScanResult {
                    queued: 0,
                    skipped: 0,
                });
            };
            vec![picked
                .into_path()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned()]
        };
        let (roots, ignored) = upload_paths::prepare(paths).await?;
        skipped += ignored;
        let mut pending = VecDeque::new();
        for root in roots {
            scanned += 1;
            if state.scan_cancel.load(Ordering::Acquire) {
                return Err("扫描已取消；已加入任务保留在传输列表".into());
            }
            if root.is_directory {
                let remote =
                    ensure_upload_folder(&current, &parent, &root.name, &mut folder_cache).await?;
                pending.push_back((root.path, remote));
            } else if supplied {
                batch.push(TransferSpec {
                    direction: TransferDirection::Upload,
                    remote: parent.clone(),
                    local_path: root.path.to_string_lossy().into_owned(),
                    name: Some(root.name),
                });
                if batch.len() >= 100 {
                    queued += state
                        .transfers
                        .enqueue(std::mem::take(&mut batch))
                        .await?
                        .len();
                    let _ = app.emit(
                        "scan-progress",
                        serde_json::json!({"queued":queued,"path":parent.name}),
                    );
                }
            }
        }
        while let Some((local, remote)) = pending.pop_front() {
            let mut entries = tokio::fs::read_dir(&local)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
                if state.scan_cancel.load(Ordering::Acquire) {
                    return Err("扫描已取消；已加入任务保留在传输列表".into());
                }
                scanned += 1;
                if scanned > 100000 {
                    return Err("目录超过 100,000 项；已加入任务可在传输列表查看".into());
                }
                let kind = entry.file_type().await.map_err(|e| e.to_string())?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if kind.is_symlink() {
                    skipped += 1;
                    continue;
                }
                if kind.is_dir() {
                    let folder =
                        ensure_upload_folder(&current, &remote, &name, &mut folder_cache).await?;
                    pending.push_back((entry.path(), folder));
                } else if kind.is_file() {
                    batch.push(TransferSpec {
                        direction: TransferDirection::Upload,
                        remote: remote.clone(),
                        local_path: entry.path().to_string_lossy().into_owned(),
                        name: Some(name),
                    });
                } else {
                    skipped += 1;
                }
                if batch.len() >= 100 {
                    let jobs = state.transfers.enqueue(std::mem::take(&mut batch)).await?;
                    queued += jobs.len();
                    let _ = app.emit(
                        "scan-progress",
                        serde_json::json!({"queued":queued,"path":remote.name}),
                    );
                }
            }
        }
    }
    #[cfg(target_os = "ios")]
    {
        return Err("iOS 文件夹上传尚未适配，请使用文件上传".into());
    }
    if !batch.is_empty() {
        queued += state.transfers.enqueue(batch).await?.len();
    }
    Ok(UploadScanResult { queued, skipped })
}
#[tauri::command]
async fn request_transfer_protection(
    app: tauri::AppHandle,
) -> Result<tauri_plugin_document_picker::ForegroundStatus, String> {
    app.document_picker().request_transfer_protection().await
}
#[tauri::command]
async fn transfer_overview(
    state: State<'_, AppState>,
) -> Result<transfer_activity::TransferOverview, String> {
    state.transfers.overview().await
}
#[tauri::command]
async fn transfer_summary(state: State<'_, AppState>) -> Result<Value, String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.summary())
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
async fn list_transfers(
    state: State<'_, AppState>,
    limit: usize,
    offset: usize,
    filter: Option<String>,
) -> Result<Vec<TransferJob>, String> {
    let transfers = state.transfers.clone();
    tauri::async_runtime::spawn_blocking(move || {
        transfers.list_filtered(limit, offset, filter.as_deref().unwrap_or("all"))
    })
    .await
    .map_err(|error| error.to_string())?
}
#[tauri::command]
async fn frontend_ready(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    #[cfg(desktop)]
    {
        if state.tray_ready.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = tauri::async_runtime::spawn_blocking(move || {
            tray::setup(&app).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
        if result.is_err() {
            state.tray_ready.store(false, Ordering::Release);
        }
        result
    }
    #[cfg(mobile)]
    {
        let _ = (app, state);
        Ok(())
    }
}
#[tauri::command]
fn control_transfer(state: State<'_, AppState>, id: String, action: String) -> Result<(), String> {
    match action.as_str() {
        "pause" => state.transfers.pause(&id),
        "resume" => state.transfers.resume(&id),
        "cancel" => state.transfers.cancel(&id),
        "retry" => state.transfers.retry(&id),
        _ => Err("不支持的传输操作".into()),
    }
}
#[tauri::command]
async fn control_transfers(
    state: State<'_, AppState>,
    action: String,
    direction: Option<String>,
) -> Result<usize, String> {
    if action == "resume" {
        let _session = state.session.lock().await;
        state
            .transfers
            .control_batch(&action, direction.as_deref())
            .await
    } else {
        state
            .transfers
            .control_batch(&action, direction.as_deref())
            .await
    }
}
#[tauri::command]
fn clear_finished(state: State<'_, AppState>) -> Result<usize, String> {
    state.transfers.remove_finished()
}
#[tauri::command]
fn remove_completed_transfer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.transfers.remove_completed(&id)
}
#[tauri::command]
fn set_concurrency(state: State<'_, AppState>, concurrency: usize) -> Result<(), String> {
    state.store.save_concurrency(concurrency)?;
    state.transfers.set_concurrency(concurrency)
}

#[tauri::command]
async fn set_video_immersive(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    app.document_picker().set_video_immersive(enabled).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // Acquire the instance lock before opening the transfer database or webview.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        tray::show(app);
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_document_picker::init())
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().tray_by_id("main").is_some() && window.hide().is_ok() {
                    api.prevent_close();
                }
            }
            #[cfg(mobile)]
            let _ = (window, event);
        })
        .setup(|app| {
            let store =
                Arc::new(Store::new(&app.path().app_data_dir()?).map_err(std::io::Error::other)?);
            app.manage(file_preview::ArchivePreviews::default());
            app.manage(Arc::new(openlist_crypt::CryptManager::default()));
            app.manage(Arc::new(
                repositories::RepositoryManager::new(
                    app.path().app_data_dir()?.join("AxuanStor"),
                    app.path().app_cache_dir()?.join("AxuanStor"),
                )
                .map_err(std::io::Error::other)?,
            ));
            let transfers = TransferManager::new(store.clone(), app.handle().clone());
            transfers.discard_previews_on_start(&app.path().app_cache_dir()?.join("AxuanStor/repository-previews")).map_err(std::io::Error::other)?;
            transfers
                .set_concurrency(store.concurrency().map_err(std::io::Error::other)?)
                .map_err(std::io::Error::other)?;
            transfers.start();
            app.manage(AppState {
                client: RwLock::new(None),
                pending_login: RwLock::new(None),
                session: Mutex::new(()),
                scan: Mutex::new(()),
                scan_cancel: Arc::new(AtomicBool::new(false)),
                #[cfg(desktop)]
                tray_ready: AtomicBool::new(false),
                store,
                transfers,
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
                    let state = handle.state::<AppState>();
                    let _session = state.session.lock().await;
                    if let Ok(current) = client(&state).await {
                        if let Ok(config) = current.refresh_credentials().await {
                            let _ = persist(&state.store, &config);
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            frontend_ready,
            connect_account,
            list_accounts,
            select_account,
            remove_account,
            get_account,
            discover_official,
            import_official,
            list_browse_sources,
            add_browse_source,
            remove_browse_source,
            update_options,
            get_options,
            get_foreground_status,
            request_transfer_protection,
            open_completed_transfer,
            export_completed_transfer,
            list_files,
            get_link,
            get_preview,
            set_video_immersive,
            file_preview::read_text_preview,
            file_preview::read_pdf_preview,
            file_preview::read_office_preview,
            file_preview::open_archive_preview,
            file_preview::close_archive_preview,
            file_preview::save_archive_member,
            file_preview::save_archive_selection,
            get_quota,
            make_directory,
            rename_file,
            remove_files,
            relocate_files,
            queue_transfers,
            queue_uploads,
            queue_upload_directory,
            queue_downloads,
            list_transfers,
            transfer_summary,
            transfer_overview,
            control_transfer,
            control_transfers,
            clear_finished,
            remove_completed_transfer,
            set_concurrency,
            cancel_scan,
            repositories::probe_repositories,
            repositories::open_repository,
            repositories::list_repository,
            repositories::close_repository,
            repositories::forget_repository_unlock,
            repositories::edit_repository,
            repositories::plan_repository_uploads,
            repositories::execute_repository_uploads,
            repositories::create_repository,
            repositories::create_named_repository,
            repositories::change_repository_unlock,
            repositories::generate_repository_key,
            repositories::download_repository_file,
            repositories::export_repository_download,
            repositories::import_repository_key
            ,repositories::cache::label_repository_cache
            ,repositories::cache::list_repository_caches
            ,repositories::cache::delete_repository_cache
            ,repositories::cache::forget_all_repository_credentials
            ,repositories::queue::queue_repository_downloads
            ,repositories::repository_file_link
            ,repositories::bridge::import_cloud_to_repository
            ,repositories::bridge::export_repository_to_cloud
            ,repositories::bridge::relocate_between_repositories
            ,repositories::preview::preview_repository_file
            ,repositories::preview::repository_preview_ready
            ,repositories::preview::release_repository_preview
            ,repositories::preview::open_repository_media_player
            ,openlist_crypt::probe_crypt_folders
            ,openlist_crypt::create_crypt_folder
            ,openlist_crypt::open_crypt_folder
            ,openlist_crypt::close_crypt_folder
            ,openlist_crypt::list_crypt_folder
            ,openlist_crypt::edit_crypt_folder
            ,openlist_crypt::batch_edit_crypt_folder
            ,openlist_crypt::update_crypt_folder_settings
            ,openlist_crypt::queue_crypt_uploads
            ,openlist_crypt::queue_crypt_upload_directory
            ,openlist_crypt::queue_crypt_downloads
            ,openlist_crypt::preview_crypt_file
            ,openlist_crypt::release_crypt_preview
            ,openlist_crypt::open_crypt_media_player
        ])
        .run(tauri::generate_context!())
        .expect("CMCloud++ failed to start");
}

#[cfg(all(test, feature = "custom-protocol"))]
mod packaging_tests {
    #[test]
    fn production_context_embeds_frontend_assets() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let index = context
            .assets()
            .get(&"index.html".into())
            .expect("production index.html must be embedded");
        let html = String::from_utf8(index.into_owned()).expect("UTF-8 index.html");
        let mut resources = 0;
        for segment in html.split('"') {
            if segment.starts_with("/assets/") {
                assert!(
                    context.assets().get(&segment.into()).is_some(),
                    "referenced production asset is missing: {segment}"
                );
                resources += 1;
            }
        }
        assert!(
            resources >= 2,
            "index.html must reference the JS and CSS bundles"
        );
    }
}
