//! Bidirectional zero-payload relocation between ordinary Mobile Cloud folders
//! and plain AxuanStor repositories (as well as across two plain repositories).
use super::{config, error, scope, scoped_write_state, RepositoryManager, Session};
use crate::{cloud::CloudFile, AppState};
use axuanstor_sdk::repository::{
    logical_path, Change, Entry, ObjectRef, Snapshot, LEGACY_FILE_MODIFIED_AT,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoChangedEvent {
    repository_id: String,
}

struct CloudImportItem {
    logical: String,
    is_dir: bool,
    cloud_id: String,
    size: u64,
    sha256: String,
    modified_at: Option<i64>,
}

fn join_logical(prefix: &str, name: &str) -> Result<String, String> {
    let raw = if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    };
    logical_path(&raw).map_err(error)
}

fn dedup_top_roots(paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("请先选择要操作的项目".into());
    }
    if paths.len() > 10_000 {
        return Err("单次操作项目过多，请分批选择".into());
    }
    let mut normalized = paths
        .iter()
        .map(|p| logical_path(p).map_err(error))
        .collect::<Result<Vec<_>, _>>()?;
    normalized.sort();
    normalized.dedup();
    let selected: HashSet<&str> = normalized.iter().map(String::as_str).collect();
    Ok(normalized
        .iter()
        .filter(|path| {
            !path
                .match_indices('/')
                .any(|(end, _)| selected.contains(&path[..end]))
        })
        .cloned()
        .collect())
}

fn ensure_destination_available(basis: &Snapshot, target: &str, is_dir: bool) -> Result<(), String> {
    match basis.resolve(target) {
        Err(axuanstor_sdk::Error::NotFound) => {}
        Ok(_) => return Err(format!("对象库目标位置已存在同名项目：{target}")),
        Err(e) => return Err(error(e)),
    }
    let mut ancestor = target;
    while let Some((parent, _)) = ancestor.rsplit_once('/') {
        match basis.resolve(parent) {
            Ok(entry) if entry.kind == "file" => {
                return Err(format!("目标上级路径是文件而非目录：{parent}"));
            }
            Ok(_) | Err(axuanstor_sdk::Error::NotFound) => {}
            Err(e) => return Err(error(e)),
        }
        ancestor = parent;
    }
    let prefix = format!("{target}/");
    for (path, versions) in basis
        .versions
        .range(prefix.clone()..)
        .take_while(|(path, _)| path.starts_with(&prefix))
    {
        if versions.iter().any(|v| v.entry.is_some()) {
            if !is_dir {
                return Err(format!("对象库目标位置已存在同名目录：{target}"));
            }
            return Err(format!("对象库目标位置已存在子项：{path}"));
        }
    }
    Ok(())
}

fn ensure_plain_writable(session: &Session) -> Result<(String, String), String> {
    let state = session.state.lock().map_err(|_| "对象库正在操作")?;
    if state.repo.descriptor.kind != "plain" {
        return Err("加密对象库不支持直接移入或移出".into());
    }
    Ok((state.repo.descriptor.id.clone(), session.folder.clone()))
}

#[tauri::command]
pub async fn import_cloud_to_repository(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    revision: Option<String>,
    destination: String,
    files: Vec<CloudFile>,
    copy: bool,
) -> Result<(), String> {
    if files.is_empty() {
        return Err("请先选择要转入对象库的文件或文件夹".into());
    }
    let client = crate::client(&state).await?;
    let cfg = client.account_config().await;
    if cfg.space != "personal_new" {
        return Err("仅支持在个人云与普通对象库之间直接流转".into());
    }
    for f in &files {
        crate::require_writable(f)?;
    }
    let owner = scope(&cfg);
    let session = manager.session(&handle, &owner)?;
    let (repo_id, repo_folder_id) = ensure_plain_writable(&session)?;
    let dest_dir = if destination.trim().is_empty() {
        String::new()
    } else {
        logical_path(destination.trim()).map_err(error)?
    };

    let mut seen_top = HashSet::new();
    let mut top_files = Vec::new();
    for file in files {
        if file.id == repo_folder_id {
            return Err("不能将对象库自身移入内部".into());
        }
        if seen_top.insert(file.id.clone()) {
            top_files.push(file);
        }
    }

    let mut collected: Vec<CloudImportItem> = Vec::new();
    let mut queue: VecDeque<(CloudFile, String)> = VecDeque::new();
    for item in &top_files {
        let target = join_logical(&dest_dir, &item.name)?;
        if item.is_dir {
            queue.push_back((item.clone(), target));
        } else {
            let (size, sha256, modified_at) = client.file_digest(&item.id).await?;
            collected.push(CloudImportItem {
                logical: target,
                is_dir: false,
                cloud_id: item.id.clone(),
                size,
                sha256,
                modified_at,
            });
        }
    }

    while let Some((folder, prefix)) = queue.pop_front() {
        if folder.id == repo_folder_id {
            return Err("所选目录包含目标对象库自身，无法移入".into());
        }
        let children = client.list_personal_entries(&folder.id).await?;
        if children.iter().any(|(c, _, _)| {
            !c.is_dir
                && (c.name == axuanstor_sdk::repository::MARKER
                    || c.name == ".openlist-crypt"
                    || c.name == "cmcloud-openlist-crypt.json")
        }) {
            return Err(format!(
                "目录「{}」是对象库或加密文件夹，不能作为普通文件夹整体移入",
                folder.name
            ));
        }
        if children.is_empty() {
            collected.push(CloudImportItem {
                logical: prefix,
                is_dir: true,
                cloud_id: folder.id,
                size: 0,
                sha256: String::new(),
                modified_at: None,
            });
            continue;
        }
        for (child, mut hash, mut modified) in children {
            let child_logical = join_logical(&prefix, &child.name)?;
            if child.is_dir {
                queue.push_back((child, child_logical));
            } else {
                let mut size = child.size;
                if hash.is_empty() {
                    let (exact_size, exact_hash, exact_mod) = client.file_digest(&child.id).await?;
                    size = exact_size;
                    hash = exact_hash;
                    if modified.is_none() {
                        modified = exact_mod;
                    }
                }
                collected.push(CloudImportItem {
                    logical: child_logical,
                    is_dir: false,
                    cloud_id: child.id,
                    size,
                    sha256: hash,
                    modified_at: modified,
                });
            }
            if collected.len() > 50_000 {
                return Err("单次转入对象库项目超过 50,000 项，请分批操作".into());
            }
        }
    }

    let s = session.clone();
    let expected_rev = revision.filter(|r| !r.is_empty());
    let writer = axuanstor_sdk::repository::digest(owner.as_bytes())[..32].to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut witness: Vec<String> = collected.iter().map(|i| i.logical.clone()).collect();
        if !dest_dir.is_empty() {
            witness.push(dest_dir.clone());
        }
        let basis = {
            let mut guard = s.state.lock().map_err(|_| "对象库正在操作")?;
            if guard.repo.descriptor.kind != "plain" {
                return Err("加密对象库不支持直接移入或移出".into());
            }
            if let Some(policy) = guard.repo.strong_policy() {
                if collected.iter().any(|i| policy.protects(&i.logical)) {
                    return Err("目标位置受强目录权限保护，不支持直接移入".into());
                }
            }
            let (basis, _) = scoped_write_state(&s, &mut guard, &witness, &[])?;
            if let Some(rev) = &expected_rev {
                if &guard.sync_revision != rev {
                    return Err("对象库目录已更新，请重新选择目标位置".into());
                }
            }
            for item in &collected {
                ensure_destination_available(&basis, &item.logical, item.is_dir)?;
            }
            basis
        };

        let mut unique_blobs: HashMap<String, (u64, String)> = HashMap::new();
        for item in &collected {
            if !item.is_dir {
                if let Some((prev_size, _)) = unique_blobs.get(&item.sha256) {
                    if *prev_size != item.size {
                        return Err("云盘文件哈希与长度不一致，已中止入库".into());
                    }
                } else {
                    unique_blobs.insert(item.sha256.clone(), (item.size, item.cloud_id.clone()));
                }
            }
        }

        let mut linked_refs: HashMap<String, ObjectRef> = HashMap::with_capacity(unique_blobs.len());
        for (sha, (size, src_id)) in unique_blobs {
            let obj = s
                .backend
                .link_existing_blob(&sha, size, Some(&src_id))
                .map_err(error)?;
            linked_refs.insert(sha, obj);
        }

        let changes: Vec<Change> = collected
            .into_iter()
            .map(|item| {
                if item.is_dir {
                    Ok(Change {
                        path: item.logical,
                        entry: Some(Entry::directory()),
                    })
                } else {
                    let obj = linked_refs
                        .get(&item.sha256)
                        .cloned()
                        .ok_or_else(|| "对象引用缺失".to_string())?;
                    Ok(Change {
                        path: item.logical,
                        entry: Some(Entry {
                            kind: "file".into(),
                            sha256: item.sha256,
                            size: item.size,
                            object: Some(obj),
                            modified_at: Some(item.modified_at.unwrap_or(LEGACY_FILE_MODIFIED_AT)),
                        }),
                    })
                }
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut guard = s.state.lock().map_err(|_| "对象库正在操作")?;
        let (fresh_basis, _) = scoped_write_state(&s, &mut guard, &witness, &[])?;
        if fresh_basis.heads != basis.heads {
            for ch in &changes {
                ensure_destination_available(
                    &fresh_basis,
                    &ch.path,
                    ch.entry.as_ref().is_some_and(|e| e.kind == "directory"),
                )?;
            }
        }
        guard
            .repo
            .commit(&fresh_basis, &writer, changes)
            .map_err(error)?;
        guard.view = None;
        guard.inventory = None;
        guard.complete_snapshot = None;
        guard.repo.save_observed_file(&s.observation).map_err(error)
    })
    .await
    .map_err(|_| "转入对象库操作已中断")??;

    let _ = app.emit(
        "repository-directory-changed",
        RepoChangedEvent {
            repository_id: repo_id,
        },
    );

    if !copy {
        let remove_ids: Vec<String> = top_files.iter().map(|f| f.id.clone()).collect();
        client.remove_personal_ids(&remove_ids).await?;
        let _ = app.emit("directory-changed", ());
    }
    Ok(())
}

struct ExportTree {
    repo_id: String,
    roots: Vec<String>,
    directories: Vec<String>,
    files: Vec<(String, u64, String, String)>,
}

#[tauri::command]
pub async fn export_repository_to_cloud(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    handle: String,
    revision: String,
    paths: Vec<String>,
    destination: CloudFile,
    copy: bool,
) -> Result<(), String> {
    if !destination.is_dir {
        return Err("导出目标必须是云盘文件夹".into());
    }
    crate::require_writable(&destination)?;
    let client = crate::client(&state).await?;
    let cfg = client.account_config().await;
    if cfg.space != "personal_new" {
        return Err("仅支持从普通对象库导出至个人云目录".into());
    }
    let owner = scope(&cfg);
    let session = manager.session(&handle, &owner)?;
    let (_, repo_folder_id) = ensure_plain_writable(&session)?;
    if destination.id == repo_folder_id {
        return Err("目标目录是对象库底层目录，请直接选择对象库内部目录".into());
    }

    let existing_dest = client.list_personal_entries(&destination.id).await?;
    if existing_dest
        .iter()
        .any(|(f, _, _)| !f.is_dir && f.name == axuanstor_sdk::repository::MARKER)
    {
        return Err("目标文件夹本身是对象库，请点击进入对象库后再执行复制或移动".into());
    }
    let existing_names: HashSet<String> = existing_dest.into_iter().map(|(f, _, _)| f.name).collect();

    let roots = dedup_top_roots(&paths)?;
    for root in &roots {
        let top_name = root.rsplit('/').next().ok_or("对象库路径无效")?;
        if existing_names.contains(top_name) {
            return Err(format!("目标云盘目录已存在同名项目：{top_name}"));
        }
    }

    let s = session.clone();
    let rev = revision.clone();
    let export_tree = tokio::task::spawn_blocking(move || -> Result<ExportTree, String> {
        let mut guard = s.state.lock().map_err(|_| "对象库正在操作")?;
        if guard.repo.descriptor.kind != "plain" {
            return Err("加密对象库不支持直接移入或移出".into());
        }
        if !copy {
            if let Some(policy) = guard.repo.strong_policy() {
                if roots.iter().any(|r| policy.protects(r)) {
                    return Err("受保护目录不支持直接移出".into());
                }
            }
        }
        let (basis, _) = scoped_write_state(&s, &mut guard, &[], &roots)?;
        if guard.sync_revision != rev {
            return Err("对象库目录视图已更新，请刷新后重试".into());
        }
        let mut dir_set: BTreeSet<String> = BTreeSet::new();
        let mut out_files: Vec<(String, u64, String, String)> = Vec::new();
        for root in &roots {
            let top_name = root.rsplit('/').next().ok_or("路径无效")?;
            let prefix = format!("{root}/");
            let mut matched_any = false;
            for (path, versions) in basis
                .versions
                .get_key_value(root)
                .into_iter()
                .chain(
                    basis
                        .versions
                        .range(prefix.clone()..)
                        .take_while(|(p, _)| p.starts_with(&prefix)),
                )
            {
                if !versions.iter().any(|v| v.entry.is_some()) {
                    continue;
                }
                let entry = basis.resolve(path).map_err(error)?;
                matched_any = true;
                let rel = if path == root {
                    top_name.to_string()
                } else {
                    format!("{top_name}/{}", &path[prefix.len()..])
                };
                let mut anc = rel.as_str();
                while let Some((parent, _)) = anc.rsplit_once('/') {
                    dir_set.insert(parent.to_string());
                    anc = parent;
                }
                if entry.kind == "directory" {
                    dir_set.insert(rel);
                } else if entry.kind == "file" {
                    let obj = entry.object.as_ref().ok_or("对象库文件缺少底层引用")?;
                    out_files.push((rel, entry.size, entry.sha256.clone(), obj.id.clone()));
                }
            }
            if !matched_any {
                return Err(format!("未找到所选对象库路径：{root}"));
            }
        }
        let mut directories: Vec<String> = dir_set.into_iter().collect();
        directories.sort_by(|a, b| {
            a.matches('/')
                .count()
                .cmp(&b.matches('/').count())
                .then_with(|| a.cmp(b))
        });
        Ok(ExportTree {
            repo_id: guard.repo.descriptor.id.clone(),
            roots,
            directories,
            files: out_files,
        })
    })
    .await
    .map_err(|_| "读取对象库导出列表已中断")??;

    let mut cloud_dirs: BTreeMap<String, CloudFile> = BTreeMap::new();
    cloud_dirs.insert(String::new(), destination.clone());
    for rel_dir in &export_tree.directories {
        let (parent_rel, name) = rel_dir.rsplit_once('/').unwrap_or(("", rel_dir.as_str()));
        let parent_cloud = cloud_dirs
            .get(parent_rel)
            .cloned()
            .ok_or("上级云盘目录创建状态缺失")?;
        let created = client.create_directory(&parent_cloud, name).await?;
        cloud_dirs.insert(rel_dir.clone(), created);
    }

    for (rel_file, size, sha256, blob_file_id) in &export_tree.files {
        let (parent_rel, name) = rel_file.rsplit_once('/').unwrap_or(("", rel_file.as_str()));
        let parent_cloud = cloud_dirs
            .get(parent_rel)
            .ok_or("目标云盘父目录不存在")?;
        client
            .materialize_dedup_file(&parent_cloud.id, name, *size, sha256, Some(blob_file_id))
            .await?;
    }

    if !copy {
        let s = session.clone();
        let writer = axuanstor_sdk::repository::digest(owner.as_bytes())[..32].to_string();
        let roots = export_tree.roots;
        let expected_rev = revision;
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut guard = s.state.lock().map_err(|_| "对象库正在操作")?;
            let (basis, _) = scoped_write_state(&s, &mut guard, &[], &roots)?;
            if guard.sync_revision != expected_rev {
                return Err("文件已导出至云盘目录，但原对象库并发变更，未自动移除原条目；请刷新后核对".into());
            }
            guard
                .repo
                .remove_many(&basis, &writer, &roots)
                .map_err(error)?;
            guard.view = None;
            guard.inventory = None;
            guard.complete_snapshot = None;
            guard.repo.save_observed_file(&s.observation).map_err(error)
        })
        .await
        .map_err(|_| "对象库清理原路径已中断")??;

        let _ = app.emit(
            "repository-directory-changed",
            RepoChangedEvent {
                repository_id: export_tree.repo_id,
            },
        );
    }
    let _ = app.emit("directory-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn relocate_between_repositories(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, Arc<RepositoryManager>>,
    source_handle: String,
    source_revision: String,
    paths: Vec<String>,
    target_handle: String,
    target_revision: Option<String>,
    destination: String,
    copy: bool,
) -> Result<(), String> {
    let cfg = config(&state).await?;
    if cfg.space != "personal_new" {
        return Err("仅支持个人云普通对象库跨库流转".into());
    }
    let owner = scope(&cfg);
    let src_session = manager.session(&source_handle, &owner)?;
    let dst_session = manager.session(&target_handle, &owner)?;
    let (src_repo_id, _) = ensure_plain_writable(&src_session)?;
    let (dst_repo_id, _) = ensure_plain_writable(&dst_session)?;
    let dest_dir = if destination.trim().is_empty() {
        String::new()
    } else {
        logical_path(destination.trim()).map_err(error)?
    };
    let roots = dedup_top_roots(&paths)?;
    if src_repo_id == dst_repo_id {
        let writer = axuanstor_sdk::repository::digest(owner.as_bytes())[..32].to_string();
        let s = src_session.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut guard = s.state.lock().map_err(|_| "对象库正在操作")?;
            let moves = roots
                .iter()
                .map(|path| {
                    let name = path.rsplit('/').next().ok_or("文件名称无效")?;
                    Ok((path.clone(), join_logical(&dest_dir, name)?))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let witness: Vec<String> = moves.iter().map(|(_, d)| d.clone()).collect();
            let (basis, _) = scoped_write_state(&s, &mut guard, &witness, &roots)?;
            if guard.sync_revision != source_revision {
                return Err("对象库目录视图已更新，请刷新后重试".into());
            }
            guard
                .repo
                .relocate_many(&basis, &writer, &moves, !copy)
                .map_err(error)?;
            guard.view = None;
            guard.repo.save_observed_file(&s.observation).map_err(error)
        })
        .await
        .map_err(|_| "对象库操作已中断")??;
        let _ = app.emit(
            "repository-directory-changed",
            RepoChangedEvent {
                repository_id: src_repo_id,
            },
        );
        return Ok(());
    }

    let writer = axuanstor_sdk::repository::digest(owner.as_bytes())[..32].to_string();
    let s_src = src_session.clone();
    let s_dst = dst_session.clone();
    let src_rev = source_revision.clone();
    let dst_rev = target_revision.filter(|r| !r.is_empty());
    let roots_for_remove = roots.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut items: Vec<(String, Option<Entry>, Option<String>)> = Vec::new();
        {
            let mut src_guard = s_src.state.lock().map_err(|_| "源对象库正在操作")?;
            let (src_basis, _) = scoped_write_state(&s_src, &mut src_guard, &[], &roots)?;
            if src_guard.sync_revision != src_rev {
                return Err("源对象库视图已更新，请刷新后重试".into());
            }
            for root in &roots {
                let top_name = root.rsplit('/').next().ok_or("路径无效")?;
                let target_root = join_logical(&dest_dir, top_name)?;
                let prefix = format!("{root}/");
                let mut found = false;
                for (path, versions) in src_basis
                    .versions
                    .get_key_value(root)
                    .into_iter()
                    .chain(
                        src_basis
                            .versions
                            .range(prefix.clone()..)
                            .take_while(|(p, _)| p.starts_with(&prefix)),
                    )
                {
                    if !versions.iter().any(|v| v.entry.is_some()) {
                        continue;
                    }
                    let entry = src_basis.resolve(path).map_err(error)?;
                    found = true;
                    let dst_path = if path == root {
                        target_root.clone()
                    } else {
                        format!("{target_root}/{}", &path[prefix.len()..])
                    };
                    if entry.kind == "directory" {
                        items.push((dst_path, Some(Entry::directory()), None));
                    } else if entry.kind == "file" {
                        let blob_id = entry.object.as_ref().map(|o| o.id.clone());
                        items.push((dst_path, Some(entry.clone()), blob_id));
                    }
                }
                if !found {
                    return Err(format!("未找到源对象库路径：{root}"));
                }
            }
        }

        let witness: Vec<String> = items.iter().map(|(p, _, _)| p.clone()).collect();
        let dst_basis = {
            let mut dst_guard = s_dst.state.lock().map_err(|_| "目标对象库正在操作")?;
            let (basis, _) = scoped_write_state(&s_dst, &mut dst_guard, &witness, &[])?;
            if let Some(rev) = &dst_rev {
                if &dst_guard.sync_revision != rev {
                    return Err("目标对象库目录已更新，请重新选择".into());
                }
            }
            for (path, entry, _) in &items {
                ensure_destination_available(
                    &basis,
                    path,
                    entry.as_ref().is_some_and(|e| e.kind == "directory"),
                )?;
            }
            basis
        };

        let mut linked: HashMap<String, ObjectRef> = HashMap::new();
        for (_, entry_opt, src_blob_id) in &items {
            if let Some(entry) = entry_opt {
                if entry.kind == "file" && !linked.contains_key(&entry.sha256) {
                    let obj = s_dst
                        .backend
                        .link_existing_blob(&entry.sha256, entry.size, src_blob_id.as_deref())
                        .map_err(error)?;
                    linked.insert(entry.sha256.clone(), obj);
                }
            }
        }

        let changes: Vec<Change> = items
            .into_iter()
            .map(|(path, entry_opt, _)| {
                let mut entry = entry_opt.unwrap();
                if entry.kind == "file" {
                    entry.object = linked.get(&entry.sha256).cloned();
                }
                Change {
                    path,
                    entry: Some(entry),
                }
            })
            .collect();

        {
            let mut dst_guard = s_dst.state.lock().map_err(|_| "目标对象库正在操作")?;
            let (fresh_basis, _) = scoped_write_state(&s_dst, &mut dst_guard, &witness, &[])?;
            if fresh_basis.heads != dst_basis.heads {
                for ch in &changes {
                    ensure_destination_available(
                        &fresh_basis,
                        &ch.path,
                        ch.entry.as_ref().is_some_and(|e| e.kind == "directory"),
                    )?;
                }
            }
            dst_guard
                .repo
                .commit(&fresh_basis, &writer, changes)
                .map_err(error)?;
            dst_guard.view = None;
            dst_guard.inventory = None;
            dst_guard.complete_snapshot = None;
            dst_guard
                .repo
                .save_observed_file(&s_dst.observation)
                .map_err(error)?;
        }

        if !copy {
            let mut src_guard = s_src.state.lock().map_err(|_| "源对象库正在操作")?;
            let (src_basis, _) = scoped_write_state(&s_src, &mut src_guard, &[], &roots_for_remove)?;
            if src_guard.sync_revision != src_rev {
                return Err("文件已复制到目标对象库，但源对象库并发变更，未自动移除原条目".into());
            }
            src_guard
                .repo
                .remove_many(&src_basis, &writer, &roots_for_remove)
                .map_err(error)?;
            src_guard.view = None;
            src_guard.inventory = None;
            src_guard.complete_snapshot = None;
            src_guard
                .repo
                .save_observed_file(&s_src.observation)
                .map_err(error)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| "跨对象库流转已中断")??;

    let _ = app.emit(
        "repository-directory-changed",
        RepoChangedEvent {
            repository_id: dst_repo_id,
        },
    );
    if !copy {
        let _ = app.emit(
            "repository-directory-changed",
            RepoChangedEvent {
                repository_id: src_repo_id,
            },
        );
    }
    Ok(())
}
