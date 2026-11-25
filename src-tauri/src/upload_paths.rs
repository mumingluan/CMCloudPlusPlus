use std::{collections::HashSet, path::PathBuf};

pub struct UploadRoot {
    pub path: PathBuf,
    pub name: String,
    pub is_directory: bool,
}

pub async fn prepare(paths: Vec<String>) -> Result<(Vec<UploadRoot>, usize), String> {
    if paths.is_empty() || paths.len() > 10_000 {
        return Err("每次请拖入 1 至 10,000 个文件或文件夹".into());
    }
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut skipped = 0;
    for value in paths {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("拖入项目必须使用本机绝对路径".into());
        }
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|e| e.to_string())?;
        if !metadata.is_file() && !metadata.is_dir() {
            skipped += 1;
            continue;
        }
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(|e| e.to_string())?;
        if !seen.insert(path.clone()) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("无法识别拖入项目的名称")?
            .to_owned();
        roots.push(UploadRoot {
            path,
            name,
            is_directory: metadata.is_dir(),
        });
    }
    // A file/folder dragged along with its ancestor belongs to that ancestor's
    // tree; do not enqueue it a second time at the destination root.
    let parents: Vec<_> = roots
        .iter()
        .filter(|root| root.is_directory)
        .map(|root| root.path.clone())
        .collect();
    roots.retain(|root| {
        !parents
            .iter()
            .any(|parent| root.path != *parent && root.path.starts_with(parent))
    });
    Ok((roots, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mixed_drops_keep_folder_structure_and_remove_overlapping_sources() {
        let root = std::env::temp_dir().join(format!("cmcloud-drop-{}", uuid::Uuid::new_v4()));
        let folder = root.join("folder");
        tokio::fs::create_dir_all(folder.join("nested"))
            .await
            .unwrap();
        let file = root.join("single.txt");
        let child = folder.join("child.txt");
        tokio::fs::write(&file, b"one").await.unwrap();
        tokio::fs::write(&child, b"two").await.unwrap();
        let (roots, skipped) = prepare(vec![
            file.to_string_lossy().into(),
            child.to_string_lossy().into(),
            folder.to_string_lossy().into(),
            file.to_string_lossy().into(),
        ])
        .await
        .unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots.iter().filter(|root| root.is_directory).count(), 1);
        assert!(roots.iter().any(|root| root.name == "single.txt"));
        assert!(!roots.iter().any(|root| root.name == "child.txt"));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn invalid_drop_paths_are_rejected_before_queueing() {
        assert!(prepare(Vec::new()).await.is_err());
        assert!(prepare(vec!["relative.txt".into()]).await.is_err());
        assert!(prepare(vec!["x".into(); 10_001]).await.is_err());
        let missing =
            std::env::temp_dir().join(format!("cmcloud-missing-{}", uuid::Uuid::new_v4()));
        assert!(prepare(vec![missing.to_string_lossy().into()])
            .await
            .is_err());
    }
}
