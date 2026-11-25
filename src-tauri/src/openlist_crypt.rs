//! OpenList-compatible encrypted folder sessions, directory operations,
//! OS-protected credential persistence, streaming media playback, and transfer queueing.

use crate::{
    cloud::{AccountConfig, CloudClient, CloudFile},
    file_client,
    openlist_crypt_cipher::{CryptFolderOptions, OpenListCipher, FILE_NONCE_SIZE},
    openlist_crypt_media::{self as crypt_media, CryptPlayback, CryptRanges},
    repositories,
    store::Store,
    system_secrets,
    transfers::{CryptTransfer, SpecialTransfer, TransferDirection, TransferJob, TransferSpec},
    AppState, UploadScanResult,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, State};
#[cfg(desktop)]
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "android")]
use tauri_plugin_document_picker::DocumentPickerExt;
#[cfg(target_os = "android")]
use tauri::Manager;
use zeroize::Zeroize;

#[derive(Default)]
pub struct CryptManager {
    sessions: Mutex<HashMap<String, Arc<CryptSession>>>,
    unlocked: Mutex<HashMap<(String, String), Arc<OpenListCipher>>>,
    playbacks: Mutex<HashMap<String, CryptPlayback>>,
}

pub struct CryptSession {
    pub scope: String,
    pub root_folder: CloudFile,
    pub cipher: Arc<OpenListCipher>,
    pub options: CryptFolderOptions,
    pub saved: AtomicBool,
    dirs: Mutex<HashMap<String, CloudFile>>,
    entries: Mutex<HashMap<String, CachedEntry>>,
}

#[derive(Clone)]
pub struct CachedEntry {
    pub item: CryptItem,
    pub raw: CloudFile,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptFolderProbe {
    pub folder_id: String,
    pub known: bool,
    pub saved: bool,
    pub options: Option<CryptFolderOptions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptItem {
    pub name: String,
    pub path: String,
    pub kind: String, // "file" | "directory"
    pub size: u64,
    pub encrypted_size: u64,
    pub modified_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptView {
    pub handle: String,
    pub folder_id: String,
    pub directory: String,
    pub items: Vec<CryptItem>,
    pub saved: bool,
    pub unrecognized_count: usize,
    pub options: CryptFolderOptions,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptPreviewStart {
    pub url: Option<String>,
    pub stream_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSecret {
    format: u8,
    password: String,
    salt: String,
}

impl Drop for StoredSecret {
    fn drop(&mut self) {
        self.password.zeroize();
        self.salt.zeroize();
    }
}

fn bindings_key(scope: &str) -> String {
    format!("openlist_crypt.bindings.{scope}")
}

pub fn secret_id(scope: &str, folder_id: &str) -> String {
    let hash = hex::encode(Sha256::digest(format!("{scope}\0{folder_id}").as_bytes()));
    format!("crypt:{hash}")
}

fn load_bindings(store: &Store, scope: &str) -> Result<HashMap<String, CryptFolderOptions>, String> {
    use rusqlite::OptionalExtension;
    let db = store.db.lock().map_err(|_| "配置存储不可用")?;
    let raw: Option<String> = db
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [bindings_key(scope)],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "读取加密文件夹配置失败")?;
    match raw {
        Some(text) => serde_json::from_str(&text).map_err(|_| "加密文件夹配置格式损坏".into()),
        None => Ok(HashMap::new()),
    }
}

fn save_bindings(
    store: &Store,
    scope: &str,
    map: &HashMap<String, CryptFolderOptions>,
) -> Result<(), String> {
    let key = bindings_key(scope);
    let db = store.db.lock().map_err(|_| "配置存储不可用")?;
    if map.is_empty() {
        db.execute("DELETE FROM settings WHERE key=?1", [key])
            .map_err(|_| "清理加密文件夹配置失败")?;
    } else {
        let json = serde_json::to_string(map).map_err(|_| "序列化加密文件夹配置失败")?;
        db.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, json],
        )
        .map_err(|_| "保存加密文件夹配置失败")?;
    }
    Ok(())
}

pub fn clean_logical_path(input: &str, allow_empty: bool) -> Result<String, String> {
    let trimmed = input.trim().trim_matches('/');
    if trimmed.is_empty() {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err("路径不能为空".into())
        };
    }
    if trimmed.len() > 4096 || trimmed.contains('\\') || trimmed.chars().any(|c| c.is_control()) {
        return Err("加密文件夹路径包含非法字符".into());
    }
    let mut parts = Vec::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.len() > 255 {
            return Err("加密文件夹路径层级无效".into());
        }
        parts.push(seg);
    }
    Ok(parts.join("/"))
}

fn validate_leaf_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.len() > 255
        || trimmed.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err("请输入有效的文件或文件夹名称".into());
    }
    Ok(trimmed.to_string())
}

impl CryptManager {
    pub fn session(&self, handle: &str, expected_scope: &str) -> Result<Arc<CryptSession>, String> {
        let map = self.sessions.lock().map_err(|_| "加密文件夹会话不可用")?;
        let session = map
            .get(handle)
            .cloned()
            .ok_or_else(|| "加密文件夹会话已关闭，请重新打开".to_string())?;
        if session.scope != expected_scope {
            return Err("当前账号已切换，请重新打开加密文件夹".into());
        }
        Ok(session)
    }

    pub async fn cipher_for_transfer(
        &self,
        store: &Store,
        app: &AppHandle,
        scope: &str,
        root_folder_id: &str,
        options: &CryptFolderOptions,
    ) -> Result<Arc<OpenListCipher>, String> {
        if let Ok(guard) = self.unlocked.lock() {
            if let Some(cipher) = guard.get(&(scope.to_string(), root_folder_id.to_string())) {
                return Ok(cipher.clone());
            }
        }
        let sid = secret_id(scope, root_folder_id);
        let Some(raw) = system_secrets::load(store, Some(app), &sid).await? else {
            return Err("请先打开解锁该加密文件夹后再继续传输任务".into());
        };
        let secret: StoredSecret =
            serde_json::from_slice(&raw).map_err(|_| "保存的加密文件夹口令已损坏")?;
        if secret.format != 1 {
            return Err("保存的加密文件夹口令版本不受支持".into());
        }
        let cipher = Arc::new(OpenListCipher::new(&secret.password, &secret.salt, options)?);
        if let Ok(mut guard) = self.unlocked.lock() {
            guard.insert((scope.to_string(), root_folder_id.to_string()), cipher.clone());
        }
        Ok(cipher)
    }

    pub fn close_handle(&self, handle: &str) {
        if let Ok(mut map) = self.sessions.lock() {
            map.remove(handle);
        }
        if let Ok(mut playbacks) = self.playbacks.lock() {
            playbacks.retain(|_, p| p.handle != handle);
        }
    }

    pub fn release_playback(&self, id: &str) -> Result<bool, String> {
        Ok(self
            .playbacks
            .lock()
            .map_err(|_| "流式预览状态不可用")?
            .remove(id)
            .is_some())
    }
}

async fn current_scope_and_client(state: &AppState) -> Result<(String, CloudClient, AccountConfig), String> {
    let cloud = state
        .client
        .read()
        .await
        .clone()
        .ok_or_else(|| "请先连接移动云盘".to_string())?;
    let cfg = cloud.account_config().await;
    let scope = repositories::scope(&cfg);
    Ok((scope, cloud, cfg))
}

async fn resolve_directory_cloud(
    session: &CryptSession,
    client: &CloudClient,
    logical_dir: &str,
) -> Result<CloudFile, String> {
    let dir = clean_logical_path(logical_dir, true)?;
    if dir.is_empty() {
        return Ok(session.root_folder.clone());
    }
    if let Some(found) = session
        .dirs
        .lock()
        .map_err(|_| "目录缓存不可用")?
        .get(&dir)
        .cloned()
    {
        return Ok(found);
    }

    let mut cur_cloud = session.root_folder.clone();
    let mut cur_logical = String::new();
    for seg in dir.split('/') {
        let next_logical = if cur_logical.is_empty() {
            seg.to_string()
        } else {
            format!("{cur_logical}/{seg}")
        };
        let cached = session
            .dirs
            .lock()
            .map_err(|_| "目录缓存不可用")?
            .get(&next_logical)
            .cloned();
        if let Some(hit) = cached {
            cur_cloud = hit;
            cur_logical = next_logical;
            continue;
        }
        let expected_enc = session.cipher.encrypt_dir_name(seg);
        let raw_items = client.list_all(&cur_cloud).await?;
        let mut matched = None;
        for mut item in raw_items {
            if !item.is_dir {
                continue;
            }
            item.source_id.clone_from(&session.root_folder.source_id);
            let is_match = item.name == expected_enc
                || session
                    .cipher
                    .decrypt_dir_name(&item.name)
                    .ok()
                    .as_deref()
                    == Some(seg);
            if is_match {
                matched = Some(item);
                break;
            }
        }
        let found = matched.ok_or_else(|| format!("未找到加密子目录：{next_logical}"))?;
        session
            .dirs
            .lock()
            .map_err(|_| "目录缓存不可用")?
            .insert(next_logical.clone(), found.clone());
        cur_cloud = found;
        cur_logical = next_logical;
    }
    Ok(cur_cloud)
}

async fn resolve_entry(
    session: &CryptSession,
    client: &CloudClient,
    logical_path: &str,
) -> Result<CachedEntry, String> {
    let clean = clean_logical_path(logical_path, false)?;
    if let Some(cached) = session
        .entries
        .lock()
        .map_err(|_| "目录项缓存不可用")?
        .get(&clean)
        .cloned()
    {
        return Ok(cached);
    }
    let parent_dir = clean
        .rsplit_once('/')
        .map(|(p, _)| p)
        .unwrap_or("");
    let _ = list_session_directory(session, client, String::new(), parent_dir).await?;
    session
        .entries
        .lock()
        .map_err(|_| "目录项缓存不可用")?
        .get(&clean)
        .cloned()
        .ok_or_else(|| format!("未找到加密文件夹项目：{clean}"))
}

async fn list_session_directory(
    session: &CryptSession,
    client: &CloudClient,
    handle: String,
    logical_dir: &str,
) -> Result<CryptView, String> {
    let dir = clean_logical_path(logical_dir, true)?;
    let cloud_dir = resolve_directory_cloud(session, client, &dir).await?;
    let raw_entries = client.list_all(&cloud_dir).await?;

    let mut items = Vec::new();
    let mut unrecognized_count = 0usize;
    let mut dir_updates = Vec::new();
    let mut entry_updates = Vec::new();

    for mut raw in raw_entries {
        raw.source_id.clone_from(&session.root_folder.source_id);
        if raw.is_dir {
            match session.cipher.decrypt_dir_name(&raw.name) {
                Ok(dec_name)
                    if !dec_name.is_empty()
                        && !dec_name.contains('/')
                        && !dec_name.contains('\\') =>
                {
                    if !session.cipher.show_hidden() && dec_name.starts_with('.') {
                        continue;
                    }
                    let item_path = if dir.is_empty() {
                        dec_name.clone()
                    } else {
                        format!("{dir}/{dec_name}")
                    };
                    let item = CryptItem {
                        name: dec_name,
                        path: item_path.clone(),
                        kind: "directory".into(),
                        size: 0,
                        encrypted_size: 0,
                        modified_at: raw.modified_at.clone(),
                    };
                    dir_updates.push((item_path.clone(), raw.clone()));
                    entry_updates.push((item_path, CachedEntry { item: item.clone(), raw }));
                    items.push(item);
                }
                _ => {
                    unrecognized_count += 1;
                }
            }
        } else {
            match (
                session.cipher.decrypt_file_name(&raw.name),
                session.cipher.decrypted_size(raw.size),
            ) {
                (Ok(dec_name), Ok(plain_size))
                    if !dec_name.is_empty()
                        && !dec_name.contains('/')
                        && !dec_name.contains('\\') =>
                {
                    if !session.cipher.show_hidden() && dec_name.starts_with('.') {
                        continue;
                    }
                    let item_path = if dir.is_empty() {
                        dec_name.clone()
                    } else {
                        format!("{dir}/{dec_name}")
                    };
                    let item = CryptItem {
                        name: dec_name,
                        path: item_path.clone(),
                        kind: "file".into(),
                        size: plain_size,
                        encrypted_size: raw.size,
                        modified_at: raw.modified_at.clone(),
                    };
                    entry_updates.push((item_path, CachedEntry { item: item.clone(), raw }));
                    items.push(item);
                }
                _ => {
                    unrecognized_count += 1;
                }
            }
        }
    }

    items.sort_by(|a, b| {
        (a.kind != "directory")
            .cmp(&(b.kind != "directory"))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    {
        let mut dirs_guard = session.dirs.lock().map_err(|_| "目录缓存不可用")?;
        for (p, cloud) in dir_updates {
            dirs_guard.insert(p, cloud);
        }
    }
    {
        let mut entries_guard = session.entries.lock().map_err(|_| "目录项缓存不可用")?;
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        entries_guard.retain(|k, _| {
            if dir.is_empty() {
                k.contains('/')
            } else {
                !k.starts_with(&prefix) || k[prefix.len()..].contains('/')
            }
        });
        for (p, entry) in entry_updates {
            entries_guard.insert(p, entry);
        }
    }

    Ok(CryptView {
        handle,
        folder_id: session.root_folder.id.clone(),
        directory: dir,
        items,
        saved: session.saved.load(Ordering::Acquire),
        unrecognized_count,
        options: session.options.clone(),
    })
}

async fn ensure_encrypted_subfolder(
    client: &CloudClient,
    cipher: &OpenListCipher,
    parent_cloud: &CloudFile,
    plain_leaf_name: &str,
    cache: &mut HashMap<String, HashMap<String, CloudFile>>,
) -> Result<CloudFile, String> {
    let clean_leaf = validate_leaf_name(plain_leaf_name)?;
    let enc_name = cipher.encrypt_dir_name(&clean_leaf);
    if !cache.contains_key(&parent_cloud.id) {
        cache.clear();
        let mut map = HashMap::new();
        for f in client.list_all(parent_cloud).await? {
            map.insert(f.name.clone(), f);
        }
        cache.insert(parent_cloud.id.clone(), map);
    }
    if let Some(existing) = cache.get(&parent_cloud.id).and_then(|m| m.get(&enc_name)) {
        if existing.is_dir {
            return Ok(existing.clone());
        }
        return Err(format!("云端已有同名加密对象：{clean_leaf}"));
    }
    let created = client.create_directory(parent_cloud, &enc_name).await?;
    cache
        .entry(parent_cloud.id.clone())
        .or_default()
        .insert(enc_name, created.clone());
    Ok(created)
}

pub(crate) fn preview_bytes(
    manager: &CryptManager,
    state_client: CloudClient,
    owner: &str,
    handle: &str,
    path: &str,
    limit: u64,
    limit_message: &'static str,
) -> Result<Vec<u8>, String> {
    let session = manager.session(handle, owner)?;
    let entry = tauri::async_runtime::block_on(resolve_entry(&session, &state_client, path))?;
    if entry.item.kind != "file" {
        return Err("不能预览文件夹".into());
    }
    if entry.item.size > limit {
        return Err(limit_message.into());
    }
    let link = tauri::async_runtime::block_on(state_client.link(&entry.raw))?;
    let ranges = CryptRanges::open(
        &link,
        entry.raw.size,
        session.cipher.clone(),
        Arc::new(AtomicBool::new(false)),
    )?;
    let mut buf = vec![0u8; entry.item.size as usize];
    ranges.read_exact_at(0, &mut buf)?;
    Ok(buf)
}

pub(crate) fn archive_ranges(
    manager: &CryptManager,
    state_client: CloudClient,
    owner: &str,
    handle: &str,
    path: &str,
    cancel: Arc<AtomicBool>,
) -> Result<(Arc<CryptRanges>, u64, String), String> {
    let session = manager.session(handle, owner)?;
    let entry = tauri::async_runtime::block_on(resolve_entry(&session, &state_client, path))?;
    if entry.item.kind != "file" || entry.item.size == 0 {
        return Err("压缩包大小不可用，请刷新目录后重试。".into());
    }
    let link = tauri::async_runtime::block_on(state_client.link(&entry.raw))?;
    let ranges = Arc::new(CryptRanges::open(
        &link,
        entry.raw.size,
        session.cipher.clone(),
        cancel,
    )?);
    Ok((ranges, entry.item.size, entry.item.name))
}

#[tauri::command]
pub async fn probe_crypt_folders(
    state: State<'_, AppState>,
    folder_ids: Vec<String>,
) -> Result<Vec<CryptFolderProbe>, String> {
    if folder_ids.is_empty() {
        return Ok(Vec::new());
    }
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let bindings = load_bindings(&state.store, &scope)?;
    let mut out = Vec::with_capacity(folder_ids.len());
    for id in folder_ids.into_iter().take(500) {
        let options = bindings.get(&id).cloned();
        let saved = if options.is_some() {
            system_secrets::exists(&state.store, &secret_id(&scope, &id)).unwrap_or(false)
        } else {
            false
        };
        out.push(CryptFolderProbe {
            folder_id: id,
            known: options.is_some(),
            saved,
            options,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn create_crypt_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    parent: CloudFile,
    name: String,
    mut password: String,
    mut salt: String,
    options: CryptFolderOptions,
    remember: bool,
) -> Result<CryptView, String> {
    crate::require_writable(&parent)?;
    let folder_name = validate_leaf_name(&name)?;
    let cipher = Arc::new(OpenListCipher::new(&password, &salt, &options)?);
    let (scope, cloud, cfg) = current_scope_and_client(&state).await?;
    if cfg.space == "share" {
        password.zeroize();
        salt.zeroize();
        return Err("分享空间不支持新建加密文件夹".into());
    }
    let folder = cloud.create_directory(&parent, &folder_name).await?;

    let mut bindings = load_bindings(&state.store, &scope)?;
    bindings.insert(folder.id.clone(), options.clone());
    save_bindings(&state.store, &scope, &bindings)?;

    if remember {
        let secret = StoredSecret {
            format: 1,
            password: std::mem::take(&mut password),
            salt: std::mem::take(&mut salt),
        };
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&secret).map_err(|_| "编码加密文件夹口令失败")?,
        );
        system_secrets::save(&state.store, Some(&app), &secret_id(&scope, &folder.id), &bytes)
            .await?;
    } else {
        password.zeroize();
        salt.zeroize();
    }

    if let Ok(mut guard) = manager.unlocked.lock() {
        guard.insert((scope.clone(), folder.id.clone()), cipher.clone());
    }
    let mut dirs = HashMap::new();
    dirs.insert(String::new(), folder.clone());
    let handle = uuid::Uuid::new_v4().to_string();
    let session = Arc::new(CryptSession {
        scope,
        root_folder: folder.clone(),
        cipher,
        options: options.clone(),
        saved: AtomicBool::new(remember),
        dirs: Mutex::new(dirs),
        entries: Mutex::new(HashMap::new()),
    });
    manager
        .sessions
        .lock()
        .map_err(|_| "加密文件夹会话不可用")?
        .insert(handle.clone(), session);

    Ok(CryptView {
        handle,
        folder_id: folder.id,
        directory: String::new(),
        items: Vec::new(),
        saved: remember,
        unrecognized_count: 0,
        options,
    })
}

#[tauri::command]
pub async fn open_crypt_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    folder: CloudFile,
    mut password: Option<String>,
    mut salt: Option<String>,
    options: Option<CryptFolderOptions>,
    use_saved: bool,
    remember: bool,
) -> Result<CryptView, String> {
    if !folder.is_dir {
        return Err("请选择文件夹以作为加密文件夹打开".into());
    }
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let cloud = file_client(&state, &folder).await?;
    let mut bindings = load_bindings(&state.store, &scope)?;
    let sid = secret_id(&scope, &folder.id);

    let effective_options = options
        .or_else(|| bindings.get(&folder.id).cloned())
        .unwrap_or_default();
    let _ = effective_options.normalized()?;

    let (pw, sl, is_saved) = if use_saved {
        let Some(raw) = system_secrets::load(&state.store, Some(&app), &sid).await? else {
            return Err("未找到已保存的加密文件夹口令，请重新输入密码".into());
        };
        let mut stored: StoredSecret =
            serde_json::from_slice(&raw).map_err(|_| "已保存的口令数据已损坏")?;
        if stored.format != 1 {
            return Err("已保存的口令格式不受支持".into());
        }
        (
            std::mem::take(&mut stored.password),
            std::mem::take(&mut stored.salt),
            true,
        )
    } else {
        let p = password.take().unwrap_or_default();
        let s = salt.take().unwrap_or_default();
        let already = system_secrets::exists(&state.store, &sid).unwrap_or(false);
        (p, s, already)
    };
    let mut pw = zeroize::Zeroizing::new(pw);
    let mut sl = zeroize::Zeroizing::new(sl);

    let cipher = Arc::new(OpenListCipher::new(&pw, &sl, &effective_options)?);

    let mut dirs = HashMap::new();
    dirs.insert(String::new(), folder.clone());
    let handle = uuid::Uuid::new_v4().to_string();
    let session = Arc::new(CryptSession {
        scope: scope.clone(),
        root_folder: folder.clone(),
        cipher: cipher.clone(),
        options: effective_options.clone(),
        saved: AtomicBool::new(is_saved),
        dirs: Mutex::new(dirs),
        entries: Mutex::new(HashMap::new()),
    });

    // Verify folder listing works before persisting new binding/credentials.
    let mut view = list_session_directory(&session, &cloud, handle.clone(), "").await?;

    bindings.insert(folder.id.clone(), effective_options.clone());
    save_bindings(&state.store, &scope, &bindings)?;

    if remember && !use_saved {
        let secret = StoredSecret {
            format: 1,
            password: std::mem::take(&mut *pw),
            salt: std::mem::take(&mut *sl),
        };
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&secret).map_err(|_| "编码加密文件夹口令失败")?,
        );
        system_secrets::save(&state.store, Some(&app), &sid, &bytes).await?;
        session.saved.store(true, Ordering::Release);
        view.saved = true;
    }

    if let Ok(mut guard) = manager.unlocked.lock() {
        guard.insert((scope, folder.id.clone()), cipher);
    }
    manager
        .sessions
        .lock()
        .map_err(|_| "加密文件夹会话不可用")?
        .insert(handle, session);

    Ok(view)
}

#[tauri::command]
pub fn close_crypt_folder(manager: State<'_, Arc<CryptManager>>, handle: String) {
    manager.close_handle(&handle);
}

#[tauri::command]
pub async fn list_crypt_folder(
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    directory: String,
    force: Option<bool>,
) -> Result<CryptView, String> {
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let session = manager.session(&handle, &scope)?;
    let cloud = file_client(&state, &session.root_folder).await?;
    if force.unwrap_or(false) && !directory.is_empty() {
        let clean = clean_logical_path(&directory, true)?;
        let _ = session.dirs.lock().map(|mut d| d.remove(&clean));
    }
    list_session_directory(&session, &cloud, handle, &directory).await
}

#[tauri::command]
pub async fn edit_crypt_folder(
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    action: String,
    path: String,
    target: Option<String>,
) -> Result<CryptView, String> {
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let session = manager.session(&handle, &scope)?;
    crate::require_writable(&session.root_folder)?;
    let cloud = file_client(&state, &session.root_folder).await?;
    let clean = clean_logical_path(&path, false)?;
    let (parent_dir, leaf) = clean
        .rsplit_once('/')
        .map(|(p, l)| (p.to_string(), l.to_string()))
        .unwrap_or_else(|| (String::new(), clean.clone()));

    match action.as_str() {
        "mkdir" => {
            let valid_leaf = validate_leaf_name(&leaf)?;
            let parent_cloud = resolve_directory_cloud(&session, &cloud, &parent_dir).await?;
            let enc_name = session.cipher.encrypt_dir_name(&valid_leaf);
            let created = cloud.create_directory(&parent_cloud, &enc_name).await?;
            session
                .dirs
                .lock()
                .map_err(|_| "目录缓存不可用")?
                .insert(clean, created);
            list_session_directory(&session, &cloud, handle, &parent_dir).await
        }
        "rename" => {
            let target_str = target.ok_or("请输入新名称")?;
            let clean_target = clean_logical_path(&target_str, false)?;
            let new_leaf = clean_target
                .rsplit_once('/')
                .map(|(_, l)| l)
                .unwrap_or(&clean_target);
            let valid_new = validate_leaf_name(new_leaf)?;
            let existing = resolve_entry(&session, &cloud, &clean).await?;
            let enc_new = if existing.item.kind == "directory" {
                session.cipher.encrypt_dir_name(&valid_new)
            } else {
                session.cipher.encrypt_file_name(&valid_new)
            };
            cloud.rename(&existing.raw, &enc_new).await?;
            if existing.item.kind == "directory" {
                if let Ok(mut d) = session.dirs.lock() {
                    let prefix = format!("{clean}/");
                    d.retain(|k, _| k != &clean && !k.starts_with(&prefix));
                }
            }
            list_session_directory(&session, &cloud, handle, &parent_dir).await
        }
        other => Err(format!("不支持的加密文件夹操作：{other}")),
    }
}

#[tauri::command]
pub async fn batch_edit_crypt_folder(
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    action: String,
    paths: Vec<String>,
    destination: Option<String>,
) -> Result<CryptView, String> {
    if paths.is_empty() {
        return Err("请先选择要操作的项目".into());
    }
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let session = manager.session(&handle, &scope)?;
    crate::require_writable(&session.root_folder)?;
    let cloud = file_client(&state, &session.root_folder).await?;

    let first_clean = clean_logical_path(&paths[0], false)?;
    let current_dir = first_clean
        .rsplit_once('/')
        .map(|(p, _)| p.to_string())
        .unwrap_or_default();

    let mut entries = Vec::with_capacity(paths.len());
    for p in &paths {
        entries.push(resolve_entry(&session, &cloud, p).await?);
    }

    match action.as_str() {
        "remove" => {
            for entry in &entries {
                cloud.remove(&entry.raw).await?;
                if entry.item.kind == "directory" {
                    if let Ok(mut d) = session.dirs.lock() {
                        let prefix = format!("{}/", entry.item.path);
                        d.retain(|k, _| k != &entry.item.path && !k.starts_with(&prefix));
                    }
                }
            }
        }
        "move" | "copy" => {
            let dest_logical = clean_logical_path(destination.as_deref().unwrap_or(""), true)?;
            for entry in &entries {
                if entry.item.kind == "directory"
                    && (dest_logical == entry.item.path
                        || dest_logical.starts_with(&format!("{}/", entry.item.path)))
                {
                    return Err("不能将文件夹移动或复制到其自身及子目录中".into());
                }
            }
            let dest_cloud = resolve_directory_cloud(&session, &cloud, &dest_logical).await?;
            for entry in &entries {
                if action == "copy" {
                    cloud.copy(&entry.raw, &dest_cloud).await?;
                } else {
                    cloud.move_file(&entry.raw, &dest_cloud).await?;
                    if entry.item.kind == "directory" {
                        if let Ok(mut d) = session.dirs.lock() {
                            let prefix = format!("{}/", entry.item.path);
                            d.retain(|k, _| k != &entry.item.path && !k.starts_with(&prefix));
                        }
                    }
                }
            }
        }
        other => return Err(format!("不支持的批量操作：{other}")),
    }

    list_session_directory(&session, &cloud, handle, &current_dir).await
}

#[tauri::command]
pub async fn update_crypt_folder_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    folder_id: String,
    mut password: Option<String>,
    mut salt: Option<String>,
    options: Option<CryptFolderOptions>,
    remember: bool,
    forget: bool,
) -> Result<CryptFolderProbe, String> {
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let mut bindings = load_bindings(&state.store, &scope)?;
    let sid = secret_id(&scope, &folder_id);

    if forget {
        let _ = system_secrets::remove(&state.store, &sid);
        bindings.remove(&folder_id);
        save_bindings(&state.store, &scope, &bindings)?;
        if let Ok(mut guard) = manager.unlocked.lock() {
            guard.remove(&(scope.clone(), folder_id.clone()));
        }
        if let Ok(mut sessions) = manager.sessions.lock() {
            sessions.retain(|_, s| !(s.scope == scope && s.root_folder.id == folder_id));
        }
        return Ok(CryptFolderProbe {
            folder_id,
            known: false,
            saved: false,
            options: None,
        });
    }

    let updated_options = options
        .or_else(|| bindings.get(&folder_id).cloned())
        .unwrap_or_default();
    let _ = updated_options.normalized()?;

    if !remember {
        let _ = system_secrets::remove(&state.store, &sid);
    } else if let Some(pw) = password.take().filter(|p| !p.is_empty()) {
        let sl = salt.take().unwrap_or_default();
        let cipher = Arc::new(OpenListCipher::new(&pw, &sl, &updated_options)?);
        let secret = StoredSecret {
            format: 1,
            password: pw,
            salt: sl,
        };
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&secret).map_err(|_| "编码口令失败")?,
        );
        system_secrets::save(&state.store, Some(&app), &sid, &bytes).await?;
        if let Ok(mut guard) = manager.unlocked.lock() {
            guard.insert((scope.clone(), folder_id.clone()), cipher);
        }
    }

    bindings.insert(folder_id.clone(), updated_options.clone());
    save_bindings(&state.store, &scope, &bindings)?;

    let saved = system_secrets::exists(&state.store, &sid).unwrap_or(false);
    if let Ok(sessions) = manager.sessions.lock() {
        for s in sessions.values() {
            if s.scope == scope && s.root_folder.id == folder_id {
                s.saved.store(saved, Ordering::Release);
            }
        }
    }

    Ok(CryptFolderProbe {
        folder_id,
        known: true,
        saved,
        options: Some(updated_options),
    })
}

fn make_upload_spec_and_intent(
    scope: &str,
    root_folder_id: &str,
    parent_cloud: &CloudFile,
    cipher: &OpenListCipher,
    options: &CryptFolderOptions,
    local_path: String,
    plain_name: String,
    plain_size: u64,
) -> Result<(TransferSpec, SpecialTransfer), String> {
    let clean_name = validate_leaf_name(&plain_name)?;
    let encrypted_name = cipher.encrypt_file_name(&clean_name);
    let enc_size = cipher.encrypted_size(plain_size);
    let mut nonce = [0u8; FILE_NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce);

    let spec = TransferSpec {
        direction: TransferDirection::Upload,
        remote: CloudFile {
            id: format!("openlist_crypt:{root_folder_id}"),
            name: clean_name.clone(),
            path: parent_cloud.id.clone(),
            is_dir: true,
            size: plain_size,
            modified_at: String::new(),
            thumbnail: None,
            source_id: None,
        },
        local_path,
        name: Some(clean_name.clone()),
    };
    let intent = SpecialTransfer::OpenListCrypt(CryptTransfer {
        kind: "openlist_crypt".into(),
        format: 1,
        owner: scope.to_string(),
        root_folder_id: root_folder_id.to_string(),
        target: parent_cloud.clone(),
        encrypted_name,
        plain_name: clean_name,
        plain_size,
        enc_size,
        options: options.clone(),
        initial_nonce_hex: Some(hex::encode(nonce)),
    });
    Ok((spec, intent))
}

#[tauri::command]
pub async fn queue_crypt_uploads(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    directory: String,
) -> Result<Vec<TransferJob>, String> {
    let (scope, cloud, cfg) = current_scope_and_client(&state).await?;
    if cfg.space == "share" {
        return Err("分享空间不支持上传".into());
    }
    let session = manager.session(&handle, &scope)?;
    crate::require_writable(&session.root_folder)?;
    let dest_cloud = resolve_directory_cloud(&session, &cloud, &directory).await?;

    #[cfg(target_os = "android")]
    let selected_files: Vec<(String, String, u64)> = {
        let picked = app.document_picker().pick_files().await?;
        let mut items = Vec::with_capacity(picked.len());
        for entry in picked {
            let f = state.transfers.open_source(&entry.path).await?;
            let len = f.metadata().await.map_err(|e| e.to_string())?.len();
            items.push((entry.path, entry.name, len));
        }
        items
    };

    #[cfg(desktop)]
    let selected_files: Vec<(String, String, u64)> = {
        let picker = app.clone();
        let picked = tokio::task::spawn_blocking(move || {
            picker
                .dialog()
                .file()
                .set_title("选择要加密上传的文件")
                .blocking_pick_files()
        })
        .await
        .map_err(|e| e.to_string())?;
        let Some(picked) = picked else {
            return Ok(Vec::new());
        };
        let mut items = Vec::with_capacity(picked.len());
        for item in picked {
            let p = item.into_path().map_err(|e| e.to_string())?;
            let meta = tokio::fs::metadata(&p).await.map_err(|e| e.to_string())?;
            if !meta.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .ok_or("文件名无效")?
                .to_string_lossy()
                .into_owned();
            items.push((p.to_string_lossy().into_owned(), name, meta.len()));
        }
        items
    };

    if selected_files.is_empty() {
        return Ok(Vec::new());
    }

    let mut specs = Vec::with_capacity(selected_files.len());
    let mut intents = Vec::with_capacity(selected_files.len());
    for (local_path, plain_name, plain_size) in selected_files {
        let (spec, intent) = make_upload_spec_and_intent(
            &scope,
            &session.root_folder.id,
            &dest_cloud,
            &session.cipher,
            &session.options,
            local_path,
            plain_name,
            plain_size,
        )?;
        specs.push(spec);
        intents.push(Some(intent));
    }
    state
        .transfers
        .enqueue_with_special(specs, intents)
        .await
}

#[tauri::command]
pub async fn queue_crypt_upload_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    directory: String,
    paths: Option<Vec<String>>,
) -> Result<UploadScanResult, String> {
    #[cfg(not(desktop))]
    if paths.is_some() {
        return Err("当前平台不支持本机路径拖拽上传".into());
    }
    let _scan = state
        .scan
        .try_lock()
        .map_err(|_| "已有目录扫描正在进行，请完成或取消后再添加".to_string())?;
    state.scan_cancel.store(false, Ordering::Release);

    let (scope, cloud, cfg) = current_scope_and_client(&state).await?;
    if cfg.space == "share" {
        return Err("分享空间不支持上传".into());
    }
    let session = manager.session(&handle, &scope)?;
    crate::require_writable(&session.root_folder)?;
    let dest_parent = resolve_directory_cloud(&session, &cloud, &directory).await?;

    let mut specs = Vec::with_capacity(100);
    let mut intents = Vec::with_capacity(100);
    let mut queued = 0usize;
    #[cfg_attr(target_os = "android", allow(unused_mut))]
    let mut skipped = 0usize;
    let mut scanned = 0usize;
    let mut folder_cache = HashMap::new();

    #[cfg(target_os = "android")]
    {
        let Some(picked_dir) = app.document_picker().pick_directory().await? else {
            return Ok(UploadScanResult {
                queued: 0,
                skipped: 0,
            });
        };
        let remote_root = ensure_encrypted_subfolder(
            &cloud,
            &session.cipher,
            &dest_parent,
            &picked_dir.name,
            &mut folder_cache,
        )
        .await?;
        let mut pending = VecDeque::from([(picked_dir.path.clone(), remote_root)]);
        let mut visited = HashSet::new();
        while let Some((local, remote)) = pending.pop_front() {
            if !visited.insert(local.clone()) {
                continue;
            }
            let mut cursor = None;
            loop {
                if state.scan_cancel.load(Ordering::Acquire) {
                    if let Some(c) = cursor.as_deref() {
                        let _ = app.document_picker().close_scan(c).await;
                    }
                    return Err("扫描已取消；已加入任务保留在传输列表".into());
                }
                let page = app
                    .document_picker()
                    .list_children(&picked_dir.path, Some(&local), cursor.as_deref())
                    .await?;
                for entry in page.entries {
                    scanned += 1;
                    if scanned > 100_000 {
                        return Err("目录超过 100,000 项；已加入任务可在传输列表查看".into());
                    }
                    if entry.is_directory {
                        let sub = ensure_encrypted_subfolder(
                            &cloud,
                            &session.cipher,
                            &remote,
                            &entry.name,
                            &mut folder_cache,
                        )
                        .await?;
                        pending.push_back((entry.path, sub));
                    } else {
                        let f = state.transfers.open_source(&entry.path).await?;
                        let len = f.metadata().await.map_err(|e| e.to_string())?.len();
                        let (spec, intent) = make_upload_spec_and_intent(
                            &scope,
                            &session.root_folder.id,
                            &remote,
                            &session.cipher,
                            &session.options,
                            entry.path,
                            entry.name,
                            len,
                        )?;
                        specs.push(spec);
                        intents.push(Some(intent));
                    }
                    if specs.len() >= 100 {
                        queued += state
                            .transfers
                            .enqueue_with_special(
                                std::mem::take(&mut specs),
                                std::mem::take(&mut intents),
                            )
                            .await?
                            .len();
                        let _ = app.emit(
                            "scan-progress",
                            serde_json::json!({"queued": queued, "path": remote.name}),
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
        let chosen_paths = if let Some(p) = paths {
            p
        } else {
            let picker = app.clone();
            let picked = tokio::task::spawn_blocking(move || {
                picker
                    .dialog()
                    .file()
                    .set_title("选择要加密上传的文件夹")
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
        let (roots, ignored) = crate::upload_paths::prepare(chosen_paths).await?;
        skipped += ignored;
        let mut pending = VecDeque::new();
        for root in roots {
            scanned += 1;
            if root.is_directory {
                let remote = ensure_encrypted_subfolder(
                    &cloud,
                    &session.cipher,
                    &dest_parent,
                    &root.name,
                    &mut folder_cache,
                )
                .await?;
                pending.push_back((root.path, remote));
            } else if supplied {
                let meta = tokio::fs::metadata(&root.path)
                    .await
                    .map_err(|e| e.to_string())?;
                let (spec, intent) = make_upload_spec_and_intent(
                    &scope,
                    &session.root_folder.id,
                    &dest_parent,
                    &session.cipher,
                    &session.options,
                    root.path.to_string_lossy().into_owned(),
                    root.name,
                    meta.len(),
                )?;
                specs.push(spec);
                intents.push(Some(intent));
            }
        }
        while let Some((local_dir, remote_dir)) = pending.pop_front() {
            let mut rd = tokio::fs::read_dir(&local_dir)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
                if state.scan_cancel.load(Ordering::Acquire) {
                    return Err("扫描已取消；已加入任务保留在传输列表".into());
                }
                scanned += 1;
                if scanned > 100_000 {
                    return Err("目录超过 100,000 项；已加入任务可在传输列表查看".into());
                }
                let ft = entry.file_type().await.map_err(|e| e.to_string())?;
                if ft.is_symlink() {
                    skipped += 1;
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if ft.is_dir() {
                    let sub = ensure_encrypted_subfolder(
                        &cloud,
                        &session.cipher,
                        &remote_dir,
                        &name,
                        &mut folder_cache,
                    )
                    .await?;
                    pending.push_back((entry.path(), sub));
                } else if ft.is_file() {
                    let meta = entry.metadata().await.map_err(|e| e.to_string())?;
                    let (spec, intent) = make_upload_spec_and_intent(
                        &scope,
                        &session.root_folder.id,
                        &remote_dir,
                        &session.cipher,
                        &session.options,
                        entry.path().to_string_lossy().into_owned(),
                        name,
                        meta.len(),
                    )?;
                    specs.push(spec);
                    intents.push(Some(intent));
                } else {
                    skipped += 1;
                }
                if specs.len() >= 100 {
                    queued += state
                        .transfers
                        .enqueue_with_special(
                            std::mem::take(&mut specs),
                            std::mem::take(&mut intents),
                        )
                        .await?
                        .len();
                    let _ = app.emit(
                        "scan-progress",
                        serde_json::json!({"queued": queued, "path": remote_dir.name}),
                    );
                }
            }
        }
    }

    if !specs.is_empty() {
        queued += state
            .transfers
            .enqueue_with_special(specs, intents)
            .await?
            .len();
    }
    Ok(UploadScanResult { queued, skipped })
}

#[tauri::command]
pub async fn queue_crypt_downloads(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    paths: Vec<String>,
) -> Result<Vec<TransferJob>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let _scan = state
        .scan
        .try_lock()
        .map_err(|_| "已有目录扫描正在进行，请完成或取消后再添加".to_string())?;
    state.scan_cancel.store(false, Ordering::Release);

    let (scope, _, _) = current_scope_and_client(&state).await?;
    let session = manager.session(&handle, &scope)?;
    let cloud = file_client(&state, &session.root_folder).await?;

    #[cfg(desktop)]
    let base_dir = {
        let picker = app.clone();
        let picked = tokio::task::spawn_blocking(move || {
            picker
                .dialog()
                .file()
                .set_title("选择解密保存目录")
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
    let base_dir = {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("Downloads");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        dir
    };

    let mut pending = VecDeque::new();
    for p in paths {
        let entry = resolve_entry(&session, &cloud, &p).await?;
        pending.push_back((entry.raw, entry.item.name, entry.item.kind == "directory", entry.item.size, base_dir.clone()));
    }

    let mut visited = HashSet::new();
    let mut assigned_paths: HashSet<PathBuf> = HashSet::new();
    let mut specs = Vec::with_capacity(100);
    let mut intents = Vec::with_capacity(100);
    let mut result = Vec::new();
    let mut scanned = 0usize;
    let mut total_queued = 0usize;

    while let Some((mut raw_file, plain_name, is_dir, plain_size, local_parent)) = pending.pop_front() {
        if state.scan_cancel.load(Ordering::Acquire) {
            return Err("扫描已取消；已加入任务保留在传输列表".into());
        }
        if !visited.insert(raw_file.id.clone()) {
            continue;
        }
        scanned += 1;
        if scanned > 100_000 {
            return Err("下载目录超过 100,000 项，请分批下载".into());
        }
        let safe_name = crate::transfers::sanitize_filename(&plain_name);
        let mut target = local_parent.join(&safe_name);
        let mut collision = 0usize;
        loop {
            #[cfg(windows)]
            let key = PathBuf::from(target.to_string_lossy().to_lowercase());
            #[cfg(not(windows))]
            let key = target.clone();
            if assigned_paths.insert(key) {
                break;
            }
            collision += 1;
            let stem = PathBuf::from(&safe_name)
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("文件")
                .to_string();
            target = local_parent.join(
                if let Some(ext) = PathBuf::from(&safe_name).extension().and_then(|x| x.to_str()) {
                    format!("{stem} ({collision}).{ext}")
                } else {
                    format!("{safe_name} ({collision})")
                },
            );
        }

        if is_dir {
            tokio::fs::create_dir_all(&target)
                .await
                .map_err(|e| e.to_string())?;
            for mut child in cloud.list_all(&raw_file).await? {
                child.source_id.clone_from(&session.root_folder.source_id);
                if child.is_dir {
                    if let Ok(dec_dir) = session.cipher.decrypt_dir_name(&child.name) {
                        if !dec_dir.is_empty() && (session.cipher.show_hidden() || !dec_dir.starts_with('.')) {
                            pending.push_back((child, dec_dir, true, 0, target.clone()));
                        }
                    }
                } else if let (Ok(dec_file), Ok(dec_size)) = (
                    session.cipher.decrypt_file_name(&child.name),
                    session.cipher.decrypted_size(child.size),
                ) {
                    if !dec_file.is_empty() && (session.cipher.show_hidden() || !dec_file.starts_with('.')) {
                        pending.push_back((child, dec_file, false, dec_size, target.clone()));
                    }
                }
            }
        } else {
            raw_file.source_id.clone_from(&session.root_folder.source_id);
            let enc_size = raw_file.size;
            let enc_name = raw_file.name.clone();
            specs.push(TransferSpec {
                direction: TransferDirection::Download,
                remote: CloudFile {
                    id: format!("openlist_crypt:{}", session.root_folder.id),
                    name: plain_name.clone(),
                    path: raw_file.id.clone(),
                    is_dir: false,
                    size: plain_size,
                    modified_at: raw_file.modified_at.clone(),
                    thumbnail: None,
                    source_id: raw_file.source_id.clone(),
                },
                local_path: target.to_string_lossy().into_owned(),
                name: Some(plain_name.clone()),
            });
            intents.push(Some(SpecialTransfer::OpenListCrypt(CryptTransfer {
                kind: "openlist_crypt".into(),
                format: 1,
                owner: scope.clone(),
                root_folder_id: session.root_folder.id.clone(),
                target: raw_file,
                encrypted_name: enc_name,
                plain_name,
                plain_size,
                enc_size,
                options: session.options.clone(),
                initial_nonce_hex: None,
            })));
            total_queued += 1;
        }

        if specs.len() >= 100 {
            let jobs = state
                .transfers
                .enqueue_with_special(std::mem::take(&mut specs), std::mem::take(&mut intents))
                .await?;
            result.extend(jobs.into_iter().take(200usize.saturating_sub(result.len())));
            let _ = app.emit(
                "scan-progress",
                serde_json::json!({"queued": total_queued, "path": local_parent.to_string_lossy()}),
            );
        }
    }

    if !specs.is_empty() {
        let jobs = state
            .transfers
            .enqueue_with_special(specs, intents)
            .await?;
        result.extend(jobs.into_iter().take(200usize.saturating_sub(result.len())));
    }
    Ok(result)
}

#[tauri::command]
pub async fn preview_crypt_file(
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    handle: String,
    path: String,
) -> Result<CryptPreviewStart, String> {
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let session = manager.session(&handle, &scope)?;
    let cloud = file_client(&state, &session.root_folder).await?;
    let entry = resolve_entry(&session, &cloud, &path).await?;
    if entry.item.kind != "file" || entry.item.size == 0 {
        return Err("此文件没有可预览的内容".into());
    }
    let mime = crypt_media::mime_for(&entry.item.name)
        .ok_or_else(|| "此格式暂不支持流式预览，请下载后打开".to_string())?;
    let link = cloud.link(&entry.raw).await?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let cipher = session.cipher.clone();
    let enc_size = entry.raw.size;
    let flag = cancelled.clone();
    let ranges = tokio::task::spawn_blocking(move || CryptRanges::open(&link, enc_size, cipher, flag))
        .await
        .map_err(|_| "流式预览准备已中断")??;

    let (stream_id, playback) = crypt_media::start_server(
        scope.clone(),
        handle.clone(),
        &entry.item.name,
        mime,
        Arc::new(ranges),
        cancelled,
    )
    .await?;
    let url = playback.url.clone();
    {
        let mut map = manager.playbacks.lock().map_err(|_| "流式预览状态不可用")?;
        if map.len() >= 8 {
            if let Some(first_key) = map.keys().next().cloned() {
                map.remove(&first_key);
            }
        }
        map.insert(stream_id.clone(), playback);
    }
    Ok(CryptPreviewStart {
        url: Some(url),
        stream_id: Some(stream_id),
    })
}

#[tauri::command]
pub fn release_crypt_preview(
    manager: State<'_, Arc<CryptManager>>,
    id: String,
) -> Result<bool, String> {
    manager.release_playback(&id)
}

#[tauri::command]
pub async fn open_crypt_media_player(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<CryptManager>>,
    id: String,
) -> Result<(), String> {
    let (scope, _, _) = current_scope_and_client(&state).await?;
    let (url, _mime) = {
        let map = manager.playbacks.lock().map_err(|_| "流式预览状态不可用")?;
        let p = map
            .get(&id)
            .filter(|p| p.owner == scope)
            .ok_or("流式预览已关闭，请重新打开")?;
        (p.url.clone(), p.mime)
    };
    #[cfg(target_os = "windows")]
    {
        use std::{path::Path, process::Command};
        let players = [
            "VideoLAN/VLC/vlc.exe",
            "MPC-BE x64/mpc-be64.exe",
            "MPC-HC/mpc-hc64.exe",
            "K-Lite Codec Pack/MPC-HC64/mpc-hc64.exe",
            "mpv/mpv.exe",
        ];
        for dir in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(std::env::var_os)
        {
            for rel in players {
                let exe = Path::new(&dir).join(rel);
                if exe.is_file() && Command::new(exe).arg(&url).spawn().is_ok() {
                    return Ok(());
                }
            }
        }
        let _ = app;
        Err("未找到可启动的 VLC、MPC 或 mpv。请安装播放器，或下载后打开。".into())
    }
    #[cfg(target_os = "android")]
    {
        app.document_picker().open_media_stream(&url, _mime).await
    }
    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_url(&url, None::<&str>).map_err(|e| e.to_string())
    }
}
