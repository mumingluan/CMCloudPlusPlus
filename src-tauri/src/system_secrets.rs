//! Explicit opt-in retention for repository unlocks.
//! Account vault migration and background credential changes are separate flows.
use crate::store::Store;
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(target_os = "android")]
use tauri_plugin_document_picker::DocumentPickerExt;
use zeroize::{Zeroize, Zeroizing};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: u8,
    id: String,
    data: String,
}
fn key(id: &str) -> Result<String, String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
    {
        return Err("安全存储标识无效".into());
    };
    Ok(format!(
        "axuanstor.os-secret.{}",
        hex::encode(Sha256::digest(id.as_bytes()))
    ))
}
pub(crate) fn exists(store: &Store, id: &str) -> Result<bool, String> {
    let db = store.db.lock().map_err(|_| "安全存储不可用")?;
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM settings WHERE key=?1)",
        [key(id)?],
        |r| r.get(0),
    )
    .map_err(|_| "安全存储不可用".into())
}
pub(crate) async fn save(
    store: &Store,
    app: Option<&tauri::AppHandle>,
    id: &str,
    data: &[u8],
) -> Result<(), String> {
    let setting = key(id)?;
    if data.is_empty() || data.len() > 32768 {
        return Err("要保存的凭据长度无效".into());
    }
    let mut document = Envelope {
        format: 1,
        id: id.into(),
        data: STANDARD.encode(data),
    };
    let plain = Zeroizing::new(serde_json::to_vec(&document).map_err(|_| "安全存储编码失败")?);
    document.data.zeroize();
    let sealed = seal(store, app, id, &plain).await?;
    let db = store.db.lock().map_err(|_| "安全存储不可用")?;
    db.execute("INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[setting,STANDARD.encode(sealed)]).map_err(|_|"安全存储写入失败")?;
    Ok(())
}
pub(crate) async fn load(
    store: &Store,
    app: Option<&tauri::AppHandle>,
    id: &str,
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let encoded: Option<String> = {
        let db = store.db.lock().map_err(|_| "安全存储不可用")?;
        db.query_row("SELECT value FROM settings WHERE key=?1", [key(id)?], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| "安全存储读取失败")?
    };
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    if encoded.len() > 262144 {
        return Err("安全存储数据长度无效".into());
    }
    let sealed = STANDARD.decode(encoded).map_err(|_| "安全存储数据已损坏")?;
    if sealed.len() > 131072 {
        return Err("安全存储数据长度无效".into());
    };
    let plain = Zeroizing::new(unseal(store, app, id, &sealed).await?);
    let mut document: Envelope =
        serde_json::from_slice(&plain).map_err(|_| "安全存储数据已损坏")?;
    if document.format != 1 || document.id != id {
        document.data.zeroize();
        return Err("安全存储数据身份不匹配".into());
    }
    let decoded = STANDARD
        .decode(&document.data)
        .map_err(|_| "安全存储数据已损坏");
    document.data.zeroize();
    let decoded = decoded?;
    if decoded.is_empty() || decoded.len() > 32768 {
        return Err("安全存储数据长度无效".into());
    };
    Ok(Some(Zeroizing::new(decoded)))
}
pub(crate) fn remove(store: &Store, id: &str) -> Result<(), String> {
    let db = store.db.lock().map_err(|_| "安全存储不可用")?;
    db.execute("DELETE FROM settings WHERE key=?1", [key(id)?])
        .map_err(|_| "安全存储删除失败")?;
    Ok(())
}

/// Erase only AxuanStor's opt-in wrappers, including saved coordinator tokens.
/// Ordinary cloud account vaults and transfer journals use separate keys.
pub(crate) fn remove_all_axuanstor(store:&Store)->Result<usize,String>{
    let mut db=store.db.lock().map_err(|_|"安全存储不可用")?;
    db.execute_batch("PRAGMA secure_delete=ON;").map_err(|_|"安全存储清理不可用")?;
    let transaction=db.transaction().map_err(|_|"安全存储正在使用")?;
    let count=transaction.execute("DELETE FROM settings WHERE key GLOB 'axuanstor.os-secret.*'",[]).map_err(|_|"凭据清除失败")?;
    transaction.commit().map_err(|_|"凭据清除未确认")?;
    let busy:i64=db.query_row("PRAGMA wal_checkpoint(TRUNCATE)",[],|row|row.get(0)).map_err(|_|"凭据已移除，请重启应用以完成本机存储清理")?;
    if busy!=0{return Err("凭据已移除，请重启应用以完成本机存储清理".into())}
    Ok(count)
}
async fn seal(
    store: &Store,
    app: Option<&tauri::AppHandle>,
    id: &str,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let _ = (app, id);
        store.encrypt(data)
    }
    #[cfg(target_os = "android")]
    {
        let _ = store;
        let data = Zeroizing::new(STANDARD.encode(data));
        let result = app
            .ok_or("系统安全存储不可用")?
            .document_picker()
            .seal_secret(id, &data)
            .await?;
        STANDARD
            .decode(result)
            .map_err(|_| "系统安全存储返回无效数据".into())
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = (store, app, id, data);
        Err("当前平台尚未接入系统安全存储，请使用本次会话解锁".into())
    }
}
async fn unseal(
    store: &Store,
    app: Option<&tauri::AppHandle>,
    id: &str,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let _ = (app, id);
        store.decrypt(data)
    }
    #[cfg(target_os = "android")]
    {
        let _ = store;
        let result = Zeroizing::new(
            app.ok_or("系统安全存储不可用")?
                .document_picker()
                .unseal_secret(id, &STANDARD.encode(data))
                .await?,
        );
        STANDARD
            .decode(&*result)
            .map_err(|_| "系统安全存储返回无效数据".into())
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = (store, app, id, data);
        Err("当前平台尚未接入系统安全存储".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bulk_removal_clears_only_axuanstor_wrappers_and_keeps_cloud_account_settings(){
        let root=tempfile::tempdir().unwrap();let store=Store::new(root.path()).unwrap();
        save(&store,None,"repo:fixture",b"synthetic-password").await.unwrap();
        save(&store,None,"coordinator:fixture",b"synthetic-token").await.unwrap();
        store.db.lock().unwrap().execute("INSERT INTO settings(key,value) VALUES('cloud.account.fixture','unchanged')",[]).unwrap();
        assert_eq!(remove_all_axuanstor(&store).unwrap(),2);
        assert!(!exists(&store,"repo:fixture").unwrap());assert!(!exists(&store,"coordinator:fixture").unwrap());
        let value:String=store.db.lock().unwrap().query_row("SELECT value FROM settings WHERE key='cloud.account.fixture'",[],|row|row.get(0)).unwrap();assert_eq!(value,"unchanged");
    }
    #[tokio::test]
    async fn dpapi_roundtrip_context_binding_and_explicit_removal() {
        let dir =
            std::env::temp_dir().join(format!("cmcloud-sdk-secret-test-{}", uuid::Uuid::new_v4()));
        let store = Store::new(&dir).unwrap();
        let value = b"fixture-password-or-token";
        save(&store, None, "repository-fixture", value)
            .await
            .unwrap();
        assert!(exists(&store, "repository-fixture").unwrap());
        assert_eq!(
            &**load(&store, None, "repository-fixture")
                .await
                .unwrap()
                .as_ref()
                .unwrap(),
            value
        );
        let encoded: String = store
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [key("repository-fixture").unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!encoded.contains("fixture-password"));
        store
            .db
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key,value) VALUES (?1,?2)",
                [key("coordinator-fixture").unwrap(), encoded],
            )
            .unwrap();
        assert!(load(&store, None, "coordinator-fixture").await.is_err());
        remove(&store, "repository-fixture").unwrap();
        assert!(load(&store, None, "repository-fixture")
            .await
            .unwrap()
            .is_none());
        drop(store);
        assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
        assert!(dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cmcloud-sdk-secret-test-"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
