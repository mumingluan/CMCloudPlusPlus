//! Only rebuildable directory/metadata caches are removable here. Observation
//! pins, private upload/credential-change journals and exports are separate.
use super::*;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    scope: String,
    repository_id: String,
    folder_hash: Option<String>,
    name: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    format: u32,
    entries: BTreeMap<String, Identity>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            format: 1,
            entries: BTreeMap::new(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSummary {
    id: String,
    name: String,
    repository_id: String,
    bytes: u64,
    busy: bool,
}

fn hex_id(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identity_id(identity: &Identity) -> String {
    digest(format!("{}\0{}", identity.scope, identity.repository_id).as_bytes())
}
fn read_catalog(manager: &RepositoryManager) -> Result<Catalog, String> {
    let path = manager.directory.join("cache-catalog.json");
    let mut catalog = if path.try_exists().map_err(|_| "本机缓存目录不可用")? {
        serde_json::from_slice::<Catalog>(&sdk::local_state::read(&path, 4 << 20).map_err(error)?)
            .map_err(|_| "本机缓存清单已损坏")?
    } else {
        Catalog::default()
    };
    if catalog.format != 1 || catalog.entries.len() > 4096 {
        return Err("本机缓存清单格式无效".into());
    }
    for (id, row) in &catalog.entries {
        if !hex_id(&row.scope, 64)
            || !hex_id(&row.repository_id, 32)
            || row.folder_hash.as_ref().is_some_and(|v| !hex_id(v, 64))
            || row.name.len() > 1024
            || id != &identity_id(row)
        {
            return Err("本机缓存清单身份无效".into());
        }
    }
    // Older builds stored GUID indexes before introducing display names.
    let views = manager.directory.join("views");
    if views.exists() {
        for scope in fs::read_dir(&views).map_err(|_| "本机缓存列表不可用")? {
            let scope = scope.map_err(|_| "本机缓存列表不可用")?;
            let owner = scope.file_name().to_string_lossy().into_owned();
            if !hex_id(&owner, 64)
                || !scope
                    .file_type()
                    .map_err(|_| "本机缓存列表不可用")?
                    .is_dir()
            {
                continue;
            }
            for entry in fs::read_dir(scope.path()).map_err(|_| "本机缓存列表不可用")? {
                let entry = entry.map_err(|_| "本机缓存列表不可用")?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(guid) = name.strip_suffix(".sqlite3") else {
                    continue;
                };
                if !hex_id(guid, 32) {
                    continue;
                }
                let identity = Identity {
                    scope: owner.clone(),
                    repository_id: guid.into(),
                    folder_hash: None,
                    name: "未命名对象库".into(),
                };
                catalog
                    .entries
                    .entry(identity_id(&identity))
                    .or_insert(identity);
                if catalog.entries.len() > 4096 {
                    return Err("本机缓存对象库过多，请分批管理".into());
                }
            }
        }
    }
    Ok(catalog)
}
fn save_catalog(manager: &RepositoryManager, catalog: &Catalog) -> Result<(), String> {
    let raw = serde_json::to_vec(catalog).map_err(|_| "缓存清单编码失败")?;
    sdk::local_state::write(&manager.directory.join("cache-catalog.json"), &raw).map_err(error)
}
pub(crate) fn reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn derived_tree(root: &Path, path: &Path) -> Result<Vec<(PathBuf, bool, u64)>, String> {
    if !path.try_exists().map_err(|_| "缓存路径不可用")? {
        return Ok(vec![]);
    }
    let canonical = root.canonicalize().map_err(|_| "缓存根目录不可用")?;
    let relative = path.strip_prefix(root).map_err(|_| "缓存路径身份无效")?;
    let mut ancestor = root.to_path_buf();
    for component in relative.components() {
        ancestor.push(component);
        if reparse(&fs::symlink_metadata(&ancestor).map_err(|_| "缓存路径不可用")?) {
            return Err("缓存路径校验未通过，未删除任何文件".into());
        }
    }
    let mut pending = vec![path.to_path_buf()];
    let mut entries = vec![];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|_| "缓存内容不可用")?;
        if reparse(&metadata)
            || !path
                .canonicalize()
                .map_err(|_| "缓存路径不可用")?
                .starts_with(&canonical)
        {
            return Err("缓存路径校验未通过，未删除任何文件".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|_| "缓存目录不可用")? {
                pending.push(entry.map_err(|_| "缓存目录不可用")?.path())
            }
        } else if !metadata.is_file() {
            return Err("缓存中存在不支持的文件类型".into());
        }
        entries.push((
            path,
            metadata.is_dir(),
            if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
        ));
        if entries.len() > 1000000 {
            return Err("缓存项目过多，请稍后重试".into());
        }
    }
    Ok(entries)
}
fn derived(
    manager: &RepositoryManager,
    row: &Identity,
) -> Result<Vec<(PathBuf, bool, u64)>, String> {
    let views = manager.directory.join("views");
    let index = views
        .join(&row.scope)
        .join(format!("{}.sqlite3", row.repository_id));
    let mut result = vec![];
    for suffix in ["", "-wal", "-shm"] {
        result.extend(derived_tree(
            &manager.directory,
            &PathBuf::from(format!("{}{suffix}", index.display())),
        )?)
    }
    if let Some(folder) = &row.folder_hash {
        let root = manager.cache.join("repository-metadata");
        result.extend(derived_tree(
            &manager.cache,
            &root.join(&row.scope).join(folder),
        )?)
    }
    Ok(result)
}
fn busy(manager: &RepositoryManager, row: &Identity) -> Result<bool, String> {
    if manager
        .active_transfers
        .lock()
        .map_err(|_| "对象库传输状态不可用")?
        .get(&(row.scope.clone(), row.repository_id.clone()))
        .copied()
        .unwrap_or(0)
        > 0
    {
        return Ok(true);
    }
    let sessions = manager.sessions.lock().map_err(|_| "对象库会话不可用")?;
    for session in sessions.values() {
        if session.scope == row.scope
            && session
                .state
                .lock()
                .map_err(|_| "对象库正在操作")?
                .repo
                .descriptor
                .id
                == row.repository_id
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[tauri::command]
pub async fn label_repository_cache(
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    name: String,
) -> Result<(), String> {
    if name.is_empty() || name.len() > 1024 {
        return Err("对象库名称无效".into());
    }
    let owner = scope(&config(&state).await?);
    let session = manager.session(&handle, &owner)?;
    let row = Identity {
        scope: owner,
        repository_id: session
            .state
            .lock()
            .map_err(|_| "对象库正在操作")?
            .repo
            .descriptor
            .id
            .clone(),
        folder_hash: Some(digest(session.folder.as_bytes())),
        name,
    };
    let _guard = manager
        .cache_catalog
        .lock()
        .map_err(|_| "本机缓存清单正在操作")?;
    let mut catalog = read_catalog(&manager)?;
    catalog.entries.insert(identity_id(&row), row);
    save_catalog(&manager, &catalog)
}
#[tauri::command]
pub async fn list_repository_caches(
    manager: State<'_, Arc<RepositoryManager>>,
) -> Result<Vec<CacheSummary>, String> {
    let manager = manager.inner().clone();
    tokio::task::spawn_blocking(move || {
        let _guard = manager
            .cache_catalog
            .lock()
            .map_err(|_| "本机缓存清单正在操作")?;
        let catalog = read_catalog(&manager)?;
        let mut result = vec![];
        for (id, row) in catalog.entries {
            let bytes = derived(&manager, &row)?
                .iter()
                .fold(0u64, |sum, entry| sum.saturating_add(entry.2));
            if bytes > 0 {
                result.push(CacheSummary {
                    id,
                    name: row.name.clone(),
                    repository_id: row.repository_id.clone(),
                    bytes,
                    busy: busy(&manager, &row)?,
                })
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(result)
    })
    .await
    .map_err(|_| "本机缓存读取已中断")?
}
#[tauri::command]
pub async fn delete_repository_cache(
    manager: State<'_, Arc<RepositoryManager>>,
    id: String,
) -> Result<(), String> {
    if !hex_id(&id, 64) {
        return Err("缓存身份无效".into());
    }
    let manager = manager.inner().clone();
    tokio::task::spawn_blocking(move || delete_cache(&manager, &id))
        .await
        .map_err(|_| "本机缓存删除已中断，请刷新列表")?
}
fn delete_cache(manager: &RepositoryManager, id: &str) -> Result<(), String> {
    let _guard = manager
        .cache_catalog
        .lock()
        .map_err(|_| "本机缓存清单正在操作")?;
    let catalog = read_catalog(manager)?;
    let row = catalog
        .entries
        .get(id)
        .ok_or("该缓存已经移除，请刷新列表")?;
    if busy(&manager, row)? {
        return Err("请先退出该对象库，再删除本机缓存".into());
    }
    let mut entries = derived(&manager, row)?;
    let index = manager
        .directory
        .join("views")
        .join(&row.scope)
        .join(&row.repository_id);
    sdk::local_state::directory(index.parent().ok_or("缓存目录不可用")?).map_err(error)?;
    let _index = sdk::local_state::lock(&index.with_extension("index.lock"))
        .map_err(|_| "该目录缓存正在使用，请稍后重试")?;
    let _compact = sdk::local_state::lock(&index.with_extension("compact.lock"))
        .map_err(|_| "对象库正在维护，请稍后重试")?;
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0.components().count()));
    for (path, directory, _) in entries {
        if directory {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
        .map_err(|_| "部分缓存未能删除，请刷新后重试")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, RepositoryManager, Identity) {
        let temp = tempfile::tempdir().unwrap();
        let manager =
            RepositoryManager::new(temp.path().join("state"), temp.path().join("cache")).unwrap();
        let identity = Identity {
            scope: "a".repeat(64),
            repository_id: "b".repeat(32),
            folder_hash: Some("c".repeat(64)),
            name: "测试对象库".into(),
        };
        let views = manager.directory.join("views").join(&identity.scope);
        fs::create_dir_all(&views).unwrap();
        fs::write(
            views.join(format!("{}.sqlite3", identity.repository_id)),
            b"derived directory state",
        )
        .unwrap();
        let metadata = manager
            .cache
            .join("repository-metadata")
            .join(&identity.scope)
            .join(identity.folder_hash.as_ref().unwrap());
        fs::create_dir_all(&metadata).unwrap();
        fs::write(metadata.join("verified.metadata"), b"derived metadata").unwrap();
        let mut catalog = Catalog::default();
        catalog
            .entries
            .insert(identity_id(&identity), identity.clone());
        save_catalog(&manager, &catalog).unwrap();
        (temp, manager, identity)
    }
    #[test]
    fn cache_clear_preserves_observation_upload_checkpoint_exports_and_other_library() {
        let (_temp, manager, identity) = fixture();
        let observed = manager.directory.join(format!(
            "{}-{}.observed.json",
            identity.scope, identity.repository_id
        ));
        fs::write(&observed, b"retained observation pins").unwrap();
        let journal = manager.directory.join("journals");
        fs::create_dir_all(&journal).unwrap();
        let journal = journal.join("original-upload.json");
        fs::write(&journal, b"retained real checkpoint").unwrap();
        let export = manager.cache.join("exports");
        fs::create_dir_all(&export).unwrap();
        let export = export.join("user-file.txt");
        fs::write(&export, b"user output").unwrap();
        let other = manager
            .directory
            .join("views")
            .join(&identity.scope)
            .join(format!("{}.sqlite3", "d".repeat(32)));
        fs::write(&other, b"another library").unwrap();
        assert!(
            derived(&manager, &identity)
                .unwrap()
                .iter()
                .map(|row| row.2)
                .sum::<u64>()
                > 0
        );
        delete_cache(&manager, &identity_id(&identity)).unwrap();
        assert!(derived(&manager, &identity).unwrap().is_empty());
        assert_eq!(fs::read(observed).unwrap(), b"retained observation pins");
        assert_eq!(fs::read(journal).unwrap(), b"retained real checkpoint");
        assert_eq!(fs::read(export).unwrap(), b"user output");
        assert_eq!(fs::read(other).unwrap(), b"another library");
    }
    #[test]
    fn cache_clear_rejects_outside_paths_and_tampered_catalog_before_deleting() {
        let (temp, manager, mut identity) = fixture();
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, b"protected").unwrap();
        assert!(derived_tree(&manager.directory, &outside).is_err());
        identity.scope = "../outside".into();
        let mut catalog = Catalog::default();
        catalog
            .entries
            .insert(identity_id(&identity), identity.clone());
        sdk::local_state::write(
            &manager.directory.join("cache-catalog.json"),
            &serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();
        assert!(delete_cache(&manager, &identity_id(&identity)).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"protected");
    }
}

#[tauri::command]
pub async fn forget_all_repository_credentials(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    confirmed: bool,
) -> Result<usize, String> {
    if !confirmed {
        return Err("请完成两次删除确认".into());
    }
    state.transfers.quiesce_repositories().await?;
    let handles = manager
        .sessions
        .lock()
        .map_err(|_| "对象库会话不可用")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for handle in handles {
        manager.close(&handle)
    }
    manager
        .keys
        .lock()
        .map_err(|_| "本机密钥选择不可用")?
        .clear();
    #[cfg(not(target_os = "android"))]
    let _ = &app;
    #[cfg(target_os = "android")]
    app.document_picker().clear_secrets().await?;
    system_secrets::remove_all_axuanstor(&state.store)
}
