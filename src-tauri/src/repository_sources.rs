use super::*;
pub(super) struct Selection {
    pub files: Vec<(String, String, Option<u64>)>,
    pub directories: Vec<String>,
    pub root: String,
    pub skipped: usize,
}
pub(super) async fn select(
    app: &AppHandle,
    state: &AppState,
    folder: bool,
    paths: Option<Vec<String>>,
) -> Result<Option<Selection>, String> {
    let mut selected = Selection {
        files: vec![],
        directories: vec![],
        root: String::new(),
        skipped: 0,
    };
    #[cfg(target_os = "android")]
    {
        if paths.is_some() {
            return Err("Android 请通过文件选择器添加文件".into());
        }
        if folder {
            let Some(root) = app.document_picker().pick_directory().await? else {
                return Ok(None);
            };
            selected.root = root.path.clone();
            let name = logical_path(&root.name).map_err(error)?;
            let mut pending = std::collections::VecDeque::from([(root.path.clone(), name)]);
            let mut seen = HashSet::new();
            while let Some((local, relative)) = pending.pop_front() {
                if !seen.insert(local.clone()) {
                    continue;
                }
                selected.directories.push(relative.clone());
                let mut cursor = None;
                loop {
                    if state.scan_cancel.load(Ordering::Acquire) {
                        if let Some(cursor) = cursor.as_deref() {
                            let _ = app.document_picker().close_scan(cursor).await;
                        }
                        return Err("扫描已取消".into());
                    }
                    let page = app
                        .document_picker()
                        .list_children(&root.path, Some(&local), cursor.as_deref())
                        .await?;
                    for entry in page.entries {
                        let name =
                            logical_path(&format!("{relative}/{}", entry.name)).map_err(error)?;
                        if entry.is_directory {
                            pending.push_back((entry.path, name))
                        } else {
                            selected.files.push((entry.path, name, entry.size));
                        }
                        if selected.files.len() + selected.directories.len() + pending.len() > 10000
                        {
                            if let Some(cursor) = page.next_cursor.as_deref() {
                                let _ = app.document_picker().close_scan(cursor).await;
                            }
                            return Err("单次最多选择 10000 个项目，请分批上传".into());
                        }
                    }
                    cursor = page.next_cursor;
                    if cursor.is_none() {
                        break;
                    }
                }
            }
        } else {
            selected.files = app
                .document_picker()
                .pick_files()
                .await?
                .into_iter()
                .map(|file| (file.path, file.name, file.size))
                .collect();
            selected.root = selected
                .files
                .first()
                .map(|row| row.0.clone())
                .unwrap_or_default();
        }
    }
    #[cfg(desktop)]
    {
        let paths = if let Some(paths) = paths {
            paths
        } else {
            let app = app.clone();
            let selection = tokio::task::spawn_blocking(move || {
                let dialog = app.dialog().file().set_title(if folder {
                    "选择上传文件夹"
                } else {
                    "选择上传到对象库的文件"
                });
                if folder {
                    dialog.blocking_pick_folder().map(|path| vec![path])
                } else {
                    dialog.blocking_pick_files()
                }
            })
            .await
            .map_err(|_| "文件选择已中断")?;
            let Some(selection) = selection else {
                return Ok(None);
            };
            selection
                .into_iter()
                .map(|path| {
                    path.into_path()
                        .map(|path| path.to_string_lossy().into_owned())
                        .map_err(|_| "所选路径无效".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        if paths.is_empty() {
            return Ok(None);
        }
        let (roots, skipped) = crate::upload_paths::prepare(paths).await?;
        selected.skipped = skipped;
        let cancelled = state.scan_cancel.clone();
        selected = tokio::task::spawn_blocking(move || -> Result<Selection, String> {
            let mut pending = std::collections::VecDeque::new();
            for root in roots {
                if selected.root.is_empty() {
                    selected.root = root.path.to_string_lossy().into_owned()
                }
                pending.push_back((root.path, root.name));
            }
            while let Some((path, relative)) = pending.pop_front() {
                if cancelled.load(Ordering::Acquire) {
                    return Err("扫描已取消".into());
                }
                let metadata = std::fs::symlink_metadata(&path).map_err(|_| "所选文件不可用")?;
                if cache::reparse(&metadata) {
                    selected.skipped += 1;
                    continue;
                }
                let relative = logical_path(&relative).map_err(error)?;
                if metadata.is_dir() {
                    selected.directories.push(relative.clone());
                    for child in std::fs::read_dir(path).map_err(|_| "无法读取所选文件夹")?
                    {
                        let child = child.map_err(|_| "所选文件夹读取中断")?;
                        let name = child
                            .file_name()
                            .into_string()
                            .map_err(|_| "文件名称不是有效文本")?;
                        pending.push_back((child.path(), format!("{relative}/{name}")));
                    }
                } else if metadata.is_file() {
                    selected.files.push((
                        path.to_string_lossy().into_owned(),
                        relative,
                        Some(metadata.len()),
                    ))
                } else {
                    selected.skipped += 1
                }
                if selected.files.len() + selected.directories.len() + pending.len() > 10000 {
                    return Err("单次最多选择 10000 个项目，请分批上传".into());
                }
            }
            Ok(selected)
        })
        .await
        .map_err(|_| "文件夹扫描已中断")??;
    }
    #[cfg(not(any(target_os = "android", desktop)))]
    {
        let _ = (app, state, folder, paths);
        return Err("当前平台尚未支持此文件选择方式".into());
    }
    let _ = state;
    if selected.files.is_empty() && selected.directories.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
}
