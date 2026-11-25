//! Repository preview sources, including authenticated local media streams.
//! Remote ciphertext is never passed to the webview's media decoder.
use super::*;
use crate::transfers::{TransferDirection, TransferJob, TransferSpec};
use tauri::Manager;

pub(crate) fn archive_ranges(
    manager: &RepositoryManager, owner: &str, handle: &str, revision: &str, path: &str,
    cancel: Arc<AtomicBool>,
) -> Result<(sdk::cmcloud::PublicRanges, u64, String), String> {
    let session = manager.session(handle, owner)?;
    let (entry, name) = {
        let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
        let path = logical_path(path).map_err(error)?;
        let basis = viewed_file_basis(&session, &mut state, revision, &path)?;
        if state.repo.descriptor.kind != "plain" {
            return Err("加密压缩包请先下载解密后打开。".into());
        }
        (basis.resolve(&path).map_err(error)?.clone(), path.rsplit('/').next().unwrap_or("").to_owned())
    };
    if entry.kind != "file" {
        return Err("请选择一个压缩包。".into());
    }
    let object = entry.object.as_ref().ok_or("文件引用不完整")?;
    let ranges = session.backend.open_ranges(object, cancel).map_err(error)?;
    Ok((ranges, entry.size, name))
}

pub(crate) fn preview_bytes(
    manager: &RepositoryManager, owner: &str, handle: &str, revision: &str, path: &str, limit: u64,
    limit_message: &str,
) -> Result<Vec<u8>, String> {
    let session = manager.session(handle, owner)?;
    let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
    let path = logical_path(path).map_err(error)?;
    let basis = viewed_file_basis(&session, &mut state, revision, &path)?;
    let entry = basis.resolve(&path).map_err(error)?;
    if entry.kind != "file" || entry.size > limit {
        return Err(limit_message.into());
    }
    let mut content = Vec::with_capacity(entry.size as usize);
    state.repo.read_file(&basis, &path, &mut content).map_err(error)?;
    Ok(content)
}

#[derive(Clone)]
pub(super) struct Preview {
    owner: String,
    path: PathBuf,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    url: Option<String>,
    job: Option<TransferJob>,
    stream_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ready {
    path: Option<String>,
    status: String,
}

#[tauri::command]
pub async fn preview_repository_file(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    revision: String,
    path: String,
) -> Result<Started, String> {
    let owner = scope(&config(&state).await?);
    let session = manager.session(&handle, &owner)?;
    let m = manager.inner().clone();
    let encrypted = session.state.lock().map_err(|_| "对象库正在操作")?.repo.descriptor.kind == "encrypted";
    if let Some(mime) = super::media::mime_for(&path).filter(|_| encrypted) {
        let (id, url) = super::media::start(m, owner, handle, revision, path, mime).await?;
        return Ok(Started { url: Some(url), job: None, stream_id: Some(id) });
    }
    let root = m.cache.join("repository-previews");
    let target_root = root.clone();
    let prepared = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let path = logical_path(&path).map_err(error)?;
        let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
        let basis = viewed_file_basis(&session, &mut state, &revision, &path)?;
        let entry = basis.resolve(&path).map_err(error)?;
        if entry.kind != "file" {
            return Err("请选择一个文件".into());
        }
        if state.repo.descriptor.kind != "encrypted" {
            let reference = entry.object.as_ref().ok_or("文件引用不完整")?.clone();
            drop(state);
            return Ok((
                Some(session.backend.download_url(&reference).map_err(error)?),
                None,
            ));
        }
        if entry.size > 2 << 30 {
            return Err("此文件超过本机预览缓存上限，请下载后打开".into());
        }
        let directory = target_root.join(uuid::Uuid::new_v4().to_string());
        sdk::local_state::directory(&directory).map_err(error)?;
        let original = path.rsplit('/').next().ok_or("文件名无效")?;
        let name = crate::transfers::sanitize_filename(original);
        let output = directory.join(&name);
        let intent =
            queue::download_intent(&session, &basis, &path, &output, &directory, true, true);
        let spec = TransferSpec {
            direction: TransferDirection::Download,
            remote: CloudFile {
                id: format!("axuanstor:{}", basis.repository),
                name: name.clone(),
                path: String::new(),
                is_dir: false,
                size: entry.size,
                modified_at: String::new(),
                thumbnail: None,
                source_id: None,
            },
            local_path: output.to_string_lossy().into_owned(),
            name: Some(format!("预览 · {name}")),
        };
        Ok((None, Some((spec, intent, output))))
    })
    .await
    .map_err(|_| "预览准备已中断")??;
    if let (Some(url), None) = prepared {
        return Ok(Started {
            url: Some(url),
            job: None,
            stream_id: None,
        });
    }
    let (_, Some((spec, intent, output))) = prepared else {
        return Err("预览身份无效".into());
    };
    let result = state
        .transfers
        .enqueue_with_repository(vec![spec], vec![Some(intent)])
        .await;
    let jobs = match result {
        Ok(jobs) => jobs,
        Err(e) => {
            if let Some(parent) = output.parent() {
                let _ = std::fs::remove_dir(parent);
            }
            return Err(e);
        }
    };
    let job = jobs.into_iter().next().ok_or("预览任务未创建")?;
    m.previews.lock().map_err(|_| "预览状态不可用")?.insert(
        job.id.clone(),
        Preview {
            owner,
            path: output,
        },
    );
    let _ = app;
    Ok(Started {
        url: None,
        job: Some(job),
        stream_id: None,
    })
}

#[tauri::command]
pub async fn repository_preview_ready(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    id: String,
) -> Result<Ready, String> {
    let owner = scope(&config(&state).await?);
    let preview = manager
        .previews
        .lock()
        .map_err(|_| "预览状态不可用")?
        .get(&id)
        .cloned()
        .ok_or("预览已关闭")?;
    if preview.owner != owner {
        return Err("请在原云盘账号中预览".into());
    }
    let job = state.transfers.get(&id)?;
    if matches!(job.status.as_str(), "failed" | "cancelled") {
        return Err(job
            .error
            .unwrap_or("预览任务已停止，请重试或下载文件".into()));
    }
    if job.status != "completed" {
        return Ok(Ready {
            path: None,
            status: job.status,
        });
    }
    let meta = std::fs::symlink_metadata(&preview.path).map_err(|_| "预览缓存已失效")?;
    if !meta.is_file() || cache::reparse(&meta) || meta.len() != job.total_bytes {
        return Err("预览输出身份无效".into());
    }
    app.asset_protocol_scope()
        .allow_file(&preview.path)
        .map_err(|_| "预览文件授权失败")?;
    Ok(Ready {
        path: Some(preview.path.to_string_lossy().into_owned()),
        status: "completed".into(),
    })
}

#[tauri::command]
pub async fn release_repository_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    id: String,
) -> Result<(), String> {
    if super::media::release(&manager, &id)? {
        return Ok(());
    }
    let preview = manager
        .previews
        .lock()
        .map_err(|_| "预览状态不可用")?
        .get(&id)
        .cloned();
    let Some(preview) = preview else {
        return Ok(());
    };
    state.transfers.cancel(&id)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while state.transfers.is_active(&id)? {
        if std::time::Instant::now() >= deadline {
            return Err("预览仍在停止，临时文件将在退出后回收".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    app.asset_protocol_scope()
        .forbid_file(&preview.path)
        .map_err(|_| "预览文件授权未能撤销")?;
    state
        .transfers
        .discard_preview(&id, &manager.cache.join("repository-previews"))?;
    manager
        .previews
        .lock()
        .map_err(|_| "预览状态不可用")?
        .remove(&id);
    Ok(())
}

#[tauri::command]
pub async fn open_repository_media_player(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    id: String,
) -> Result<(), String> {
    let owner = scope(&config(&state).await?);
    let (url, mime) = super::media::external_source(&manager, &owner, &id)?;
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_document_picker::DocumentPickerExt;
        app.document_picker().open_media_stream(&url, mime).await
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (app, mime);
        super::media::open_windows_player(&url)
    }
    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    {
        let _ = (app, url, mime);
        Err("当前平台无法打开外部流式播放器".into())
    }
}
