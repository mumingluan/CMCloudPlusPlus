#[cfg(not(target_os = "windows"))]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
#[cfg(not(target_os = "windows"))]
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// All database work is serialized on short transactions; transfer bytes never enter SQLite.
pub struct Store {
    pub(crate) db: Mutex<Connection>,
    directory: PathBuf,
    vault_lock: Mutex<()>,
}

impl Store {
    pub fn new(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let db =
            Connection::open(directory.join("transfers.sqlite3")).map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS transfers (
                id TEXT PRIMARY KEY, direction TEXT NOT NULL, remote_json TEXT NOT NULL,
                local_path TEXT NOT NULL, name TEXT NOT NULL, total_bytes INTEGER NOT NULL,
                completed_bytes INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL,
                error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0, upload_state TEXT, account_scope TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS transfer_status_created ON transfers(status, created_at);
            CREATE INDEX IF NOT EXISTS transfer_path_status ON transfers(local_path, status);
            UPDATE transfers SET status='queued', error=NULL WHERE status='running';")
            .map_err(|e| e.to_string())?;
        let has_scope = db
            .prepare("PRAGMA table_info(transfers)")
            .map_err(|e| e.to_string())?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|name| name == "account_scope");
        if !has_scope {
            db.execute(
                "ALTER TABLE transfers ADD COLUMN account_scope TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(|e| e.to_string())?;
        }
        db.execute("CREATE INDEX IF NOT EXISTS transfer_scope_status ON transfers(account_scope,status,created_at)", []).map_err(|e|e.to_string())?;
        db.execute_batch("CREATE INDEX IF NOT EXISTS transfer_history_created ON transfers(created_at DESC);
            CREATE INDEX IF NOT EXISTS transfer_active_priority ON transfers(CASE status WHEN 'running' THEN 0 WHEN 'failed' THEN 1 WHEN 'paused' THEN 2 ELSE 3 END,created_at DESC) WHERE status NOT IN ('completed','cancelled');
            CREATE INDEX IF NOT EXISTS transfer_completed_created ON transfers(created_at DESC) WHERE status IN ('completed','cancelled');").map_err(|e|e.to_string())?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(db),
            directory: directory.to_path_buf(),
            vault_lock: Mutex::new(()),
        })
    }

    pub fn concurrency(&self) -> Result<usize, String> {
        use rusqlite::OptionalExtension;
        let db = self.db.lock().map_err(|_| "设置存储不可用".to_string())?;
        let value: Option<String> = db
            .query_row(
                "SELECT value FROM settings WHERE key='concurrency'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(value
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|n| (1..=8).contains(n))
            .unwrap_or(4))
    }

    pub fn save_concurrency(&self, concurrency: usize) -> Result<(), String> {
        if !(1..=8).contains(&concurrency) {
            return Err("并发数需要在 1–8 之间".into());
        }
        let db = self.db.lock().map_err(|_| "设置存储不可用".to_string())?;
        db.execute("INSERT INTO settings(key,value) VALUES ('concurrency',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [concurrency.to_string()]).map_err(|e|e.to_string())?;
        Ok(())
    }

    pub fn save_account<T: Serialize>(&self, account: &T) -> Result<(), String> {
        let _lock = self
            .vault_lock
            .lock()
            .map_err(|_| "凭据存储不可用".to_string())?;
        let plaintext = serde_json::to_vec(account).map_err(|e| e.to_string())?;
        let ciphertext = self.encrypt(&plaintext)?;
        atomic_write(&self.directory.join("account.vault"), &ciphertext)
    }

    pub fn load_account<T: DeserializeOwned>(&self) -> Result<Option<T>, String> {
        let _lock = self
            .vault_lock
            .lock()
            .map_err(|_| "凭据存储不可用".to_string())?;
        let path = self.directory.join("account.vault");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let plaintext = self.decrypt(&bytes)?;
        serde_json::from_slice(&plaintext)
            .map(Some)
            .map_err(|_| "保存的凭据格式无效，请重新登录".to_string())
    }

    pub fn delete_account(&self) -> Result<(), String> {
        let _lock = self
            .vault_lock
            .lock()
            .map_err(|_| "凭据存储不可用".to_string())?;
        let path = self.directory.join("account.vault");
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn encrypt(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        protect_windows(bytes, false)
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn decrypt(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        protect_windows(bytes, true)
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn encrypt(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let key = self.mobile_key()?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "凭据加密初始化失败".to_string())?;
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let mut output = b"CMP1".to_vec();
        output.extend_from_slice(&nonce);
        output.extend(
            cipher
                .encrypt(Nonce::from_slice(&nonce), bytes)
                .map_err(|_| "凭据加密失败".to_string())?,
        );
        Ok(output)
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn decrypt(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if bytes.len() < 32 || &bytes[..4] != b"CMP1" {
            return Err("保存的凭据已损坏，请重新登录".into());
        }
        let key = self.mobile_key()?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|_| "凭据解密初始化失败".to_string())?;
        cipher
            .decrypt(Nonce::from_slice(&bytes[4..16]), &bytes[16..])
            .map_err(|_| "凭据解密失败，请重新登录".into())
    }

    // Android/iOS use the application's private data directory. The wrapping key stays
    // within that sandbox. This protects copied databases, but is not hardware keystore protection.
    #[cfg(not(target_os = "windows"))]
    fn mobile_key(&self) -> Result<[u8; 32], String> {
        let path = self.directory.join("vault.key");
        if path.exists() {
            let bytes = fs::read(path).map_err(|e| e.to_string())?;
            return bytes.try_into().map_err(|_| "凭据密钥已损坏".into());
        }
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(mut file) => {
                file.write_all(&key).map_err(|e| e.to_string())?;
                file.sync_all().map_err(|e| e.to_string())?;
                Ok(key)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => self.mobile_key(),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn summary(&self) -> Result<serde_json::Value, String> {
        let db = self.db.lock().map_err(|_| "传输数据库不可用".to_string())?;
        let mut statement = db.prepare("SELECT status, COUNT(*), COALESCE(SUM(total_bytes),0), COALESCE(SUM(completed_bytes),0) FROM transfers GROUP BY status")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "status": row.get::<_, String>(0)?, "count":row.get::<_, u64>(1)?,
                    "totalBytes":row.get::<_, u64>(2)?, "completedBytes":row.get::<_, u64>(3)?
                }))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map(|rows| serde_json::json!(rows))
            .map_err(|e| e.to_string())
    }

    pub fn clear_completed(&self) -> Result<usize, String> {
        self.db
            .lock()
            .map_err(|_| "传输数据库不可用".to_string())?
            .execute(
                "DELETE FROM transfers WHERE status IN ('completed','cancelled')",
                params![],
            )
            .map_err(|e| e.to_string())
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let temporary = path.with_extension("vault.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    // Windows cannot replace an existing file with std::fs::rename; use an atomic
    // replace so the previous vault remains valid until the write is committed.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(not(target_os = "windows"))]
    fs::rename(temporary, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn protect_windows(bytes: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| "凭据过大".to_string())?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let result = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if result == 0 {
        return Err("Windows 无法保护或解密凭据，请在当前 Windows 用户下重新登录".into());
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(protected)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("cmcloud-store-test-{}", uuid::Uuid::new_v4()));
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn account_vault_roundtrip_is_encrypted_and_authenticated() {
        let directory = TestDirectory::new();
        let store = Store::new(&directory.0).unwrap();
        let value =
            serde_json::json!({"authorization":"secret-auth-token","password":"password-value"});
        store.save_account(&value).unwrap();
        let ciphertext = fs::read(directory.0.join("account.vault")).unwrap();
        assert!(!String::from_utf8_lossy(&ciphertext).contains("secret-auth-token"));
        assert_eq!(
            store.load_account::<serde_json::Value>().unwrap(),
            Some(value)
        );
        let mut damaged = ciphertext;
        let last = damaged.len() - 1;
        damaged[last] ^= 0x7f;
        fs::write(directory.0.join("account.vault"), damaged).unwrap();
        assert!(store.load_account::<serde_json::Value>().is_err());
    }

    #[test]
    fn multi_account_migration_survives_restart() {
        let directory = TestDirectory::new();
        {
            let store = Store::new(&directory.0).unwrap();
            store
                .save_account(&crate::cloud::AccountConfig {
                    id: "legacy-account".into(),
                    authorization: "old-credential".into(),
                    ..Default::default()
                })
                .unwrap();
            let mut vault = crate::sessions::Sessions::load(&store).unwrap();
            assert_eq!(vault.active_id.as_deref(), Some("legacy-account"));
            vault
                .upsert(crate::cloud::AccountConfig {
                    id: "second".into(),
                    authorization: "second-credential".into(),
                    ..Default::default()
                })
                .unwrap();
            vault.active_id = None;
            vault.save(&store).unwrap();
        }
        let store = Store::new(&directory.0).unwrap();
        let vault = crate::sessions::Sessions::load(&store).unwrap();
        assert_eq!(vault.accounts.len(), 2);
        assert_eq!(vault.accounts[0].authorization, "old-credential");
        assert!(vault.active_id.is_none());
        let encrypted = fs::read(directory.0.join("account.vault")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("credential"));
    }

    #[test]
    fn restart_recovers_ten_thousand_jobs_and_tb_offsets() {
        let directory = TestDirectory::new();
        {
            let store = Store::new(&directory.0).unwrap();
            let mut db = store.db.lock().unwrap();
            let transaction = db.transaction().unwrap();
            {
                let mut statement=transaction.prepare("INSERT INTO transfers(id,direction,remote_json,local_path,name,total_bytes,completed_bytes,status,created_at,updated_at,account_scope) VALUES(?,'download','{}',?,'backup',2199023255552,1099511627776,'running',0,0,'test-scope')").unwrap();
                for i in 0..10_000 {
                    statement
                        .execute(params![format!("job-{i}"), format!("/backup/{i}")])
                        .unwrap();
                }
            }
            transaction.commit().unwrap();
        }
        let store = Store::new(&directory.0).unwrap();
        let db = store.db.lock().unwrap();
        let (count, offset): (usize, u64) = db
            .query_row(
                "SELECT COUNT(*),MIN(completed_bytes) FROM transfers WHERE status='queued'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 10_000);
        assert_eq!(offset, 1_099_511_627_776);
        let running: usize = db
            .query_row(
                "SELECT COUNT(*) FROM transfers WHERE status='running'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(running, 0);
    }
}
