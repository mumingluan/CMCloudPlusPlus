//! Read-only import from the current user's official mCloud desktop profile.
//!
//! Discovery reads paths and application version only. Credential reads happen
//! exclusively when the caller invokes import; secrets are never serializable.
//! Chromium's LevelDB is read as files rather than opened as a database, so the
//! official application's locks, journal, and data are never modified.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_STORE_FILE: u64 = 64 * 1024 * 1024;
const MAX_BLOCK: usize = 16 * 1024 * 1024;
const BLOCK_SIZE: usize = 32768;
const TABLE_MAGIC: u64 = 0xdb4775248b80fb57;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialDiscovery {
    pub installed: bool,
    pub supported: bool,
    pub install_path: Option<String>,
    pub profile_path: Option<String>,
    pub version: Option<String>,
    pub can_import: bool,
    pub storage_format: String,
    pub message: String,
}

// Intentionally no Debug or Serialize: this object must remain in Rust.
pub struct ImportedCredentials {
    pub authorization: String,
    pub mail_cookies: Option<String>,
    pub username: Option<String>,
    pub user_domain_id: Option<String>,
}

fn profile_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("APPDATA") {
        candidates.push(PathBuf::from(path).join("mCloud"));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("Library/Application Support/mCloud"));
        candidates.push(home.join(".config/mCloud"));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(path).join("mCloud"));
    }
    candidates
}

fn install_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(r"C:\mCloud")];
    for name in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(path) = env::var_os(name) {
            let path = PathBuf::from(path);
            candidates.push(path.join("mCloud"));
            candidates.push(path.join("Programs/mCloud"));
        }
    }
    candidates.push(PathBuf::from("/Applications/mCloud.app/Contents"));
    candidates
}

pub fn discover_official() -> Result<OfficialDiscovery, String> {
    let supported = !cfg!(any(target_os = "android", target_os = "ios"));
    let profile = profile_candidates().into_iter().find(|p| p.is_dir());
    let install = install_candidates()
        .into_iter()
        .find(|p| p.join("mCloud.exe").is_file() || p.join("Resources/app.asar").is_file());
    let version = install.as_ref().and_then(|p| {
        let text = fs::read_to_string(p.join("config/version.txt")).ok()?;
        let value = text.trim();
        (value.len() <= 64 && value.chars().all(|c| c.is_ascii_digit() || c == '.'))
            .then(|| value.to_owned())
    });
    let can_import = supported
        && profile
            .as_ref()
            .is_some_and(|p| p.join("Local Storage/leveldb").is_dir());
    Ok(OfficialDiscovery {
        installed: profile.is_some() || install.is_some(),
        supported,
        install_path: install.map(|p| p.to_string_lossy().into_owned()),
        profile_path: profile.map(|p| p.to_string_lossy().into_owned()),
        version,
        can_import,
        storage_format: "Chromium LevelDB UserInfo（只读）".into(),
        message: if !supported {
            "移动端请使用短信、密码或手动凭据登录。".into()
        } else if can_import {
            "已找到官方客户端。点击复用后，将只读导入当前登录会话并验证有效性。".into()
        } else {
            "未找到可导入的官方客户端会话。请先在官方客户端登录一次。".into()
        },
    })
}

pub fn import_official_credentials() -> Result<ImportedCredentials, String> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return Err("官方客户端凭据复用仅支持桌面端。".into());
    }
    let profile = profile_candidates()
        .into_iter()
        .find(|p| p.join("Local Storage/leveldb").is_dir())
        .ok_or("未找到官方客户端，请先安装并登录中国移动云盘。")?;
    import_from_profile(&profile)
}

fn import_from_profile(profile: &Path) -> Result<ImportedCredentials, String> {
    let states = read_leveldb(&profile.join("Local Storage/leveldb"))?;
    // Only the active UserInfo key is eligible. Historical fromUserInfo and
    // cutoverInfo values can outlive logout and must never be imported.
    let entry = states
        .iter()
        .filter(|(key, _)| {
            key.starts_with(b"_file://\0") && local_key(key).as_deref() == Some("UserInfo")
        })
        .max_by_key(|(_, state)| state.sequence)
        .and_then(|(_, state)| state.value.as_ref())
        .ok_or("官方客户端当前没有登录会话，请先登录后再复用。")?;
    let info: Value = serde_json::from_str(&chromium_string(entry)?)
        .map_err(|_| "官方客户端会话格式已变化，请使用其他登录方式。")?;
    let account = info
        .get("account")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.contains(':'))
        .ok_or("官方客户端会话缺少账号，请重新登录官方客户端。")?;
    let token = info
        .get("authToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| info.get("token").and_then(Value::as_str))
        .filter(|s| !s.is_empty() && s.len() <= 16384 && !s.contains(['\r', '\n']))
        .ok_or("官方客户端会话缺少授权信息，请重新登录官方客户端。")?;
    if !token.contains('|') {
        return Err("官方客户端授权格式已变化，请使用其他登录方式。".into());
    }
    Ok(ImportedCredentials {
        authorization: STANDARD.encode(format!("pc:{account}:{token}")),
        // Cloud authorization is sufficient for cloud API access. Do not read
        // unrelated browser/mail cookies as part of desktop session reuse.
        mail_cookies: None,
        username: Some(account.to_owned()),
        user_domain_id: info
            .get("userDomainId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    })
}

fn local_key(key: &[u8]) -> Option<String> {
    let separator = key.iter().position(|&b| b == 0)?;
    chromium_string(key.get(separator + 1..)?).ok()
}

fn chromium_string(bytes: &[u8]) -> Result<String, String> {
    let (&encoding, data) = bytes.split_first().ok_or("客户端存储数据为空。")?;
    match encoding {
        1 => Ok(data.iter().map(|&b| char::from(b)).collect()),
        0 if data.len() % 2 == 0 => {
            let units: Vec<u16> = data
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| "客户端存储字符编码无效。".into())
        }
        _ => Err("客户端存储字符编码不受支持。".into()),
    }
}

struct StoredValue {
    sequence: u64,
    value: Option<Vec<u8>>,
}

type Store = HashMap<Vec<u8>, StoredValue>;

fn apply(store: &mut Store, key: &[u8], sequence: u64, value: Option<&[u8]>) {
    // Avoid retaining unrelated application/browser data in process memory.
    if !key.starts_with(b"_file://\0") || local_key(key).as_deref() != Some("UserInfo") {
        return;
    }
    if store
        .get(key)
        .is_none_or(|current| sequence > current.sequence)
    {
        store.insert(
            key.to_vec(),
            StoredValue {
                sequence,
                value: value.map(<[u8]>::to_vec),
            },
        );
    }
}

fn read_leveldb(directory: &Path) -> Result<Store, String> {
    let active = active_leveldb_files(directory)?;
    let mut store = Store::new();
    let mut readable = 0;
    for path in &active.files {
        // Snapshot a bounded file through a read-only handle. A live writer may
        // append an incomplete final record, which the log parser ignores.
        let file = fs::File::open(&path)
            .map_err(|_| "官方客户端会话正在更新，请关闭官方客户端后重试。")?;
        let metadata = file
            .metadata()
            .map_err(|_| "无法读取官方客户端会话文件。")?;
        if metadata.len() > MAX_STORE_FILE {
            return Err("官方客户端会话存储过大，请使用其他登录方式。".into());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        if file
            .take(MAX_STORE_FILE + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_STORE_FILE
        {
            return Err("官方客户端会话正在更新，请关闭官方客户端后重试。".into());
        }
        let mut snapshot = Store::new();
        let result = if path.extension().and_then(|x| x.to_str()) == Some("log") {
            parse_log(&bytes, &mut snapshot)
        } else {
            parse_table(&bytes, &mut snapshot)
        };
        // A damaged later file may contain a logout tombstone. Fail closed
        // instead of falling back to an earlier credential snapshot.
        result?;
        for (key, state) in snapshot {
            apply(&mut store, &key, state.sequence, state.value.as_deref());
        }
        readable += 1;
    }
    if readable == 0 {
        return Err("暂时无法读取官方客户端会话，请关闭官方客户端后重试。".into());
    }
    if read_snapshot(&directory.join("CURRENT"), 256)? != active.current
        || fs::metadata(&active.manifest_path)
            .map_err(|_| "官方客户端会话清单正在更新，请关闭官方客户端后重试。")?
            .len()
            != active.manifest_size
    {
        return Err("官方客户端会话正在更新，请关闭官方客户端后重试。".into());
    }
    Ok(store)
}

struct ActiveLevelDbFiles {
    files: Vec<PathBuf>,
    current: Vec<u8>,
    manifest_path: PathBuf,
    manifest_size: u64,
}

#[derive(Default)]
struct ManifestVersion {
    tables: HashSet<(u64, u64)>,
    log_number: Option<u64>,
    previous_log_number: u64,
    next_file_number: Option<u64>,
    last_sequence: Option<u64>,
}

fn read_snapshot(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file =
        fs::File::open(path).map_err(|_| "无法读取官方客户端会话清单，请关闭官方客户端后重试。")?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "官方客户端会话清单正在更新，请关闭官方客户端后重试。")?;
    if bytes.len() as u64 > limit {
        return Err("官方客户端会话清单过大，请使用其他登录方式。".into());
    }
    Ok(bytes)
}

fn active_leveldb_files(directory: &Path) -> Result<ActiveLevelDbFiles, String> {
    let current = read_snapshot(&directory.join("CURRENT"), 256)?;
    let name = std::str::from_utf8(&current)
        .map_err(|_| "官方客户端 CURRENT 格式无效。")?
        .trim_end_matches(['\r', '\n']);
    let number = name.strip_prefix("MANIFEST-").unwrap_or_default();
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err("官方客户端 CURRENT 清单名称无效。".into());
    }
    let manifest_path = directory.join(name);
    let manifest = read_snapshot(&manifest_path, MAX_STORE_FILE)?;
    let mut version = ManifestVersion::default();
    visit_log_records(&manifest, true, |record| {
        apply_manifest_edit(record, &mut version)
    })?;
    let log_number = version.log_number.ok_or("官方客户端清单缺少日志编号。")?;
    if version.next_file_number.is_none() || version.last_sequence.is_none() {
        return Err("官方客户端会话清单不完整。".into());
    }
    let mut missing_tables: HashSet<u64> =
        version.tables.iter().map(|(_, number)| *number).collect();
    let mut tables: HashMap<u64, PathBuf> = HashMap::new();
    let mut files = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|_| "无法读取官方客户端会话目录，请检查文件权限。")?
    {
        let path = entry
            .map_err(|_| "无法读取官方客户端会话文件列表。")?
            .path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !stem.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(number) = stem.parse::<u64>() else {
            continue;
        };
        match path.extension().and_then(|s| s.to_str()) {
            Some("log") if number >= log_number || number == version.previous_log_number => {
                files.push(path)
            }
            Some("ldb" | "sst") if missing_tables.contains(&number) => {
                // LevelDB prefers .ldb and supports the older .sst spelling.
                if !tables.contains_key(&number) || path.extension().is_some_and(|s| s == "ldb") {
                    tables.insert(number, path);
                }
            }
            _ => {}
        }
    }
    for (number, path) in tables {
        missing_tables.remove(&number);
        files.push(path);
    }
    if !missing_tables.is_empty() {
        return Err("官方客户端有效会话文件正在更新或缺失，请关闭官方客户端后重试。".into());
    }
    files.sort();
    Ok(ActiveLevelDbFiles {
        files,
        current,
        manifest_path,
        manifest_size: manifest.len() as u64,
    })
}

fn manifest_level(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let level = varint(bytes, cursor)?;
    if level >= 7 {
        return Err("官方客户端清单层级无效。".into());
    }
    Ok(level)
}

fn apply_manifest_edit(bytes: &[u8], version: &mut ManifestVersion) -> Result<(), String> {
    let mut cursor = 0;
    while cursor < bytes.len() {
        match varint(bytes, &mut cursor)? {
            1 => {
                let _ = length_prefixed(bytes, &mut cursor)?;
            }
            2 => version.log_number = Some(varint(bytes, &mut cursor)?),
            3 => version.next_file_number = Some(varint(bytes, &mut cursor)?),
            4 => version.last_sequence = Some(varint(bytes, &mut cursor)?),
            5 => {
                manifest_level(bytes, &mut cursor)?;
                let _ = length_prefixed(bytes, &mut cursor)?;
            }
            6 => {
                let level = manifest_level(bytes, &mut cursor)?;
                let number = varint(bytes, &mut cursor)?;
                version.tables.remove(&(level, number));
            }
            7 => {
                let level = manifest_level(bytes, &mut cursor)?;
                let number = varint(bytes, &mut cursor)?;
                let _size = varint(bytes, &mut cursor)?;
                let _smallest = length_prefixed(bytes, &mut cursor)?;
                let _largest = length_prefixed(bytes, &mut cursor)?;
                version.tables.insert((level, number));
            }
            9 => version.previous_log_number = varint(bytes, &mut cursor)?,
            _ => return Err("官方客户端会话清单格式不受支持。".into()),
        }
    }
    Ok(())
}

fn parse_log(bytes: &[u8], store: &mut Store) -> Result<(), String> {
    visit_log_records(bytes, false, |record| parse_batch(record, store))
}

fn visit_log_records(
    bytes: &[u8],
    strict_tail: bool,
    mut visit: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut pending = Vec::new();
    let mut collecting = false;
    for block in bytes.chunks(BLOCK_SIZE) {
        let mut cursor = 0;
        while cursor + 7 <= block.len() {
            let expected = u32::from_le_bytes(block[cursor..cursor + 4].try_into().unwrap());
            let length =
                u16::from_le_bytes(block[cursor + 4..cursor + 6].try_into().unwrap()) as usize;
            let kind = block[cursor + 6];
            cursor += 7;
            if length == 0 && kind == 0 {
                break;
            }
            if cursor + length > block.len() {
                if strict_tail {
                    return Err("官方客户端会话清单尚未写入完整，请关闭官方客户端后重试。".into());
                }
                break;
            }
            let payload = &block[cursor..cursor + length];
            cursor += length;
            if masked_crc(kind, payload) != expected {
                return Err("客户端会话日志校验失败，请关闭官方客户端后重试。".into());
            }
            match kind {
                1 => {
                    if strict_tail && collecting {
                        return Err("官方客户端会话清单分片不完整。".into());
                    }
                    pending.clear();
                    collecting = false;
                    visit(payload)?;
                }
                2 => {
                    if strict_tail && collecting {
                        return Err("官方客户端会话清单分片不完整。".into());
                    }
                    pending.clear();
                    pending.extend_from_slice(payload);
                    collecting = true;
                }
                3 | 4 if collecting => {
                    if pending.len() + payload.len() > MAX_BLOCK {
                        return Err("客户端存储记录过大。".into());
                    }
                    pending.extend_from_slice(payload);
                    if kind == 4 {
                        visit(&pending)?;
                        pending.clear();
                        collecting = false;
                    }
                }
                3 | 4 if strict_tail => return Err("官方客户端会话清单分片无效。".into()),
                3 | 4 => {}
                _ => return Err("客户端存储日志格式不受支持。".into()),
            }
        }
        if strict_tail && block[cursor..].iter().any(|b| *b != 0) {
            return Err("官方客户端会话清单尾部不完整，请关闭官方客户端后重试。".into());
        }
    }
    if strict_tail && collecting {
        return Err("官方客户端会话清单尚未写入完整，请关闭官方客户端后重试。".into());
    }
    Ok(())
}

fn parse_batch(batch: &[u8], store: &mut Store) -> Result<(), String> {
    if batch.len() < 12 {
        return Err("客户端存储批次不完整。".into());
    }
    let sequence = u64::from_le_bytes(batch[..8].try_into().unwrap());
    let count = u32::from_le_bytes(batch[8..12].try_into().unwrap()) as usize;
    if count > batch.len() {
        return Err("客户端存储批次长度无效。".into());
    }
    let mut cursor = 12;
    // Commit a batch only after all records have been parsed successfully.
    let mut mutations = Vec::new();
    for index in 0..count {
        let tag = *batch.get(cursor).ok_or("客户端存储记录不完整。")?;
        cursor += 1;
        let key = length_prefixed(batch, &mut cursor)?;
        let value = match tag {
            0 => None,
            1 => Some(length_prefixed(batch, &mut cursor)?),
            _ => return Err("客户端存储记录类型无效。".into()),
        };
        let record_sequence = sequence
            .checked_add(index as u64)
            .ok_or("客户端存储序列号无效。")?;
        mutations.push((key, record_sequence, value));
    }
    for (key, sequence, value) in mutations {
        apply(store, key, sequence, value);
    }
    Ok(())
}

fn varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or("客户端存储整数不完整。")?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err("客户端存储整数溢出。".into());
        }
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
    }
    Err("客户端存储整数无效。".into())
}

fn length_prefixed<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], String> {
    let length = usize::try_from(varint(bytes, cursor)?).map_err(|_| "客户端存储长度无效。")?;
    let end = cursor.checked_add(length).ok_or("客户端存储长度溢出。")?;
    let value = bytes.get(*cursor..end).ok_or("客户端存储记录不完整。")?;
    *cursor = end;
    Ok(value)
}

fn block_handle(bytes: &[u8], cursor: &mut usize) -> Result<(usize, usize), String> {
    let offset = usize::try_from(varint(bytes, cursor)?).map_err(|_| "客户端存储偏移无效。")?;
    let length = usize::try_from(varint(bytes, cursor)?).map_err(|_| "客户端存储长度无效。")?;
    Ok((offset, length))
}

fn table_block(bytes: &[u8], handle: (usize, usize)) -> Result<Vec<u8>, String> {
    let (offset, length) = handle;
    if length > MAX_BLOCK {
        return Err("客户端存储块过大。".into());
    }
    let end = offset.checked_add(length).ok_or("客户端存储偏移溢出。")?;
    let trailer_end = end.checked_add(5).ok_or("客户端存储偏移溢出。")?;
    let region = bytes
        .get(offset..trailer_end)
        .ok_or("客户端存储表不完整。")?;
    let content = &region[..length];
    let compression = region[length];
    let expected = u32::from_le_bytes(region[length + 1..].try_into().unwrap());
    if masked_crc_after(content, compression) != expected {
        return Err("客户端存储表校验失败。".into());
    }
    match compression {
        0 => Ok(content.to_vec()),
        1 => snappy_decode(content),
        _ => Err("客户端存储压缩方式不受支持。".into()),
    }
}

fn block_entries(bytes: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
    if bytes.len() < 4 {
        return Err("客户端存储块不完整。".into());
    }
    let count = u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().unwrap()) as usize;
    let tail = count
        .checked_mul(4)
        .and_then(|x| x.checked_add(4))
        .ok_or("客户端存储索引溢出。")?;
    let end = bytes
        .len()
        .checked_sub(tail)
        .ok_or("客户端存储索引无效。")?;
    let entries = &bytes[..end];
    let mut cursor = 0;
    let mut previous = Vec::new();
    let mut result = Vec::new();
    while cursor < end {
        let shared = usize::try_from(varint(entries, &mut cursor)?).map_err(|_| "存储键过大。")?;
        let nonshared =
            usize::try_from(varint(entries, &mut cursor)?).map_err(|_| "存储键过大。")?;
        let value_len =
            usize::try_from(varint(entries, &mut cursor)?).map_err(|_| "存储值过大。")?;
        if shared > previous.len() {
            return Err("客户端存储前缀无效。".into());
        }
        let key_end = cursor.checked_add(nonshared).ok_or("存储键溢出。")?;
        let value_end = key_end.checked_add(value_len).ok_or("存储值溢出。")?;
        let suffix = entries.get(cursor..key_end).ok_or("存储键不完整。")?;
        let value = entries.get(key_end..value_end).ok_or("存储值不完整。")?;
        previous.truncate(shared);
        previous.extend_from_slice(suffix);
        result.push((previous.clone(), value.to_vec()));
        cursor = value_end;
    }
    Ok(result)
}

fn parse_table(bytes: &[u8], store: &mut Store) -> Result<(), String> {
    if bytes.len() < 48
        || u64::from_le_bytes(bytes[bytes.len() - 8..].try_into().unwrap()) != TABLE_MAGIC
    {
        return Err("客户端存储表格式无效。".into());
    }
    let footer = &bytes[bytes.len() - 48..bytes.len() - 8];
    let mut cursor = 0;
    let _metadata = block_handle(footer, &mut cursor)?;
    let index = table_block(bytes, block_handle(footer, &mut cursor)?)?;
    let mut mutations = Vec::new();
    for (_, handle) in block_entries(&index)? {
        let mut position = 0;
        let data = table_block(bytes, block_handle(&handle, &mut position)?)?;
        for (key, value) in block_entries(&data)? {
            if key.len() < 8 {
                return Err("客户端存储内部键无效。".into());
            }
            let trailer = u64::from_le_bytes(key[key.len() - 8..].try_into().unwrap());
            let kind = trailer as u8;
            if kind > 1 {
                return Err("客户端存储内部键类型无效。".into());
            }
            let user_key = key[..key.len() - 8].to_vec();
            mutations.push((user_key, trailer >> 8, (kind == 1).then_some(value)));
        }
    }
    for (key, sequence, value) in mutations {
        apply(store, &key, sequence, value.as_deref());
    }
    Ok(())
}

fn snappy_decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut cursor = 0;
    let length = usize::try_from(varint(bytes, &mut cursor)?).map_err(|_| "压缩存储长度无效。")?;
    if length > MAX_BLOCK {
        return Err("客户端压缩存储块过大。".into());
    }
    let mut output = Vec::with_capacity(length);
    while cursor < bytes.len() && output.len() < length {
        let tag = bytes[cursor];
        cursor += 1;
        if tag & 3 == 0 {
            let mut literal = usize::from(tag >> 2);
            if literal < 60 {
                literal += 1;
            } else {
                let count = literal - 59;
                let mut encoded = 0_u32;
                for shift in 0..count {
                    encoded |=
                        u32::from(*bytes.get(cursor).ok_or("压缩存储不完整。")?) << (shift * 8);
                    cursor += 1;
                }
                literal = (encoded as usize)
                    .checked_add(1)
                    .ok_or("压缩存储长度溢出。")?;
            }
            let end = cursor.checked_add(literal).ok_or("压缩存储长度溢出。")?;
            if output.len() + literal > length {
                return Err("压缩存储长度无效。".into());
            }
            output.extend_from_slice(bytes.get(cursor..end).ok_or("压缩存储不完整。")?);
            cursor = end;
        } else {
            let (count, distance) = match tag & 3 {
                1 => {
                    let low = usize::from(*bytes.get(cursor).ok_or("压缩存储不完整。")?);
                    cursor += 1;
                    (
                        4 + usize::from((tag >> 2) & 7),
                        (usize::from(tag & 0xe0) << 3) | low,
                    )
                }
                kind => {
                    let width = if kind == 2 { 2 } else { 4 };
                    let mut distance = 0_u32;
                    for shift in 0..width {
                        distance |=
                            u32::from(*bytes.get(cursor).ok_or("压缩存储不完整。")?) << (shift * 8);
                        cursor += 1;
                    }
                    (1 + usize::from(tag >> 2), distance as usize)
                }
            };
            if distance == 0 || distance > output.len() || output.len() + count > length {
                return Err("压缩存储回引用无效。".into());
            }
            for _ in 0..count {
                output.push(output[output.len() - distance]);
            }
        }
    }
    if output.len() != length || cursor != bytes.len() {
        return Err("压缩存储长度无效。".into());
    }
    Ok(output)
}

fn crc32c(parts: &[&[u8]]) -> u32 {
    let mut crc = !0_u32;
    for part in parts {
        for byte in *part {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0x82f63b78 & (0_u32.wrapping_sub(crc & 1)));
            }
        }
    }
    !crc
}

fn mask(crc: u32) -> u32 {
    crc.rotate_right(15).wrapping_add(0xa282ead8)
}
fn masked_crc(kind: u8, bytes: &[u8]) -> u32 {
    mask(crc32c(&[&[kind], bytes]))
}
fn masked_crc_after(bytes: &[u8], kind: u8) -> u32 {
    mask(crc32c(&[bytes, &[kind]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureDirectory(PathBuf);
    impl FixtureDirectory {
        fn new() -> Self {
            let path =
                env::temp_dir().join(format!("cmcloud-manifest-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn put_varint(mut value: u64, output: &mut Vec<u8>) {
        while value >= 128 {
            output.push((value as u8 & 127) | 128);
            value >>= 7;
        }
        output.push(value as u8);
    }

    fn batch(sequence: u64, tag: u8, value: &[u8]) -> Vec<u8> {
        let key = b"_file://\0\x01UserInfo";
        let mut out = sequence.to_le_bytes().to_vec();
        out.extend_from_slice(&1_u32.to_le_bytes());
        out.push(tag);
        put_varint(key.len() as u64, &mut out);
        out.extend_from_slice(key);
        if tag == 1 {
            put_varint(value.len() as u64, &mut out);
            out.extend_from_slice(value);
        }
        out
    }

    fn record(kind: u8, data: &[u8]) -> Vec<u8> {
        let mut out = masked_crc(kind, data).to_le_bytes().to_vec();
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.push(kind);
        out.extend_from_slice(data);
        out
    }

    fn table_entry(key: &[u8], value: &[u8]) -> Vec<u8> {
        let mut out = vec![0];
        put_varint(key.len() as u64, &mut out);
        put_varint(value.len() as u64, &mut out);
        out.extend_from_slice(key);
        out.extend_from_slice(value);
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes());
        out
    }

    fn append_block(out: &mut Vec<u8>, content: &[u8]) -> Vec<u8> {
        let mut handle = Vec::new();
        put_varint(out.len() as u64, &mut handle);
        put_varint(content.len() as u64, &mut handle);
        out.extend_from_slice(content);
        out.push(0);
        out.extend_from_slice(&masked_crc_after(content, 0).to_le_bytes());
        handle
    }

    fn fixture_table(sequence: u64, kind: u8, value: &[u8]) -> Vec<u8> {
        let mut internal_key = b"_file://\0\x01UserInfo".to_vec();
        internal_key.extend_from_slice(&((sequence << 8) | u64::from(kind)).to_le_bytes());
        let mut out = Vec::new();
        let data_handle = append_block(&mut out, &table_entry(&internal_key, value));
        let index_handle = append_block(&mut out, &table_entry(&internal_key, &data_handle));
        let mut footer = vec![0, 0];
        footer.extend(index_handle);
        footer.resize(40, 0);
        footer.extend_from_slice(&TABLE_MAGIC.to_le_bytes());
        out.extend(footer);
        out
    }

    fn fixture_empty_table() -> Vec<u8> {
        let mut out = Vec::new();
        let data_handle = append_block(&mut out, &[0, 0, 0, 0, 1, 0, 0, 0]);
        let index_handle = append_block(&mut out, &table_entry(b"", &data_handle));
        let mut footer = vec![0, 0];
        footer.extend(index_handle);
        footer.resize(40, 0);
        footer.extend_from_slice(&TABLE_MAGIC.to_le_bytes());
        out.extend(footer);
        out
    }

    fn fixture_manifest() -> Vec<u8> {
        // Standard VersionEdit: log 9, next file 11, sequence 21, previous log 0,
        // and a single live table 6 at level 2 with empty key boundaries.
        record(1, &[2, 9, 3, 11, 4, 21, 9, 0, 7, 2, 6, 0, 0, 0])
    }

    fn write_manifest_fixture(directory: &Path) {
        fs::write(directory.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
        fs::write(directory.join("MANIFEST-000001"), fixture_manifest()).unwrap();
        fs::write(directory.join("000006.ldb"), fixture_empty_table()).unwrap();
        fs::write(directory.join("000009.log"), []).unwrap();
    }

    #[test]
    fn obsolete_table_cannot_restore_a_compacted_logout() {
        let directory = FixtureDirectory::new();
        write_manifest_fixture(&directory.0);
        fs::write(
            directory.0.join("000005.ldb"),
            fixture_table(20, 1, b"\x01obsolete-session"),
        )
        .unwrap();
        // Compaction removed the live value and its tombstone. A retained old
        // table must not be eligible just because its bytes still exist.
        assert!(read_leveldb(&directory.0).unwrap().is_empty());
    }

    #[test]
    fn newer_wal_is_recovered_and_obsolete_wal_is_ignored() {
        let directory = FixtureDirectory::new();
        write_manifest_fixture(&directory.0);
        fs::write(
            directory.0.join("000004.log"),
            record(1, &batch(100, 1, b"\x01obsolete-session")),
        )
        .unwrap();
        fs::write(
            directory.0.join("000010.log"),
            record(1, &batch(22, 0, b"")),
        )
        .unwrap();
        let store = read_leveldb(&directory.0).unwrap();
        let state = store.values().next().unwrap();
        assert_eq!(state.sequence, 22);
        assert!(state.value.is_none());
    }

    #[test]
    fn missing_live_file_or_incomplete_manifest_fails_closed() {
        let directory = FixtureDirectory::new();
        write_manifest_fixture(&directory.0);
        fs::remove_file(directory.0.join("000006.ldb")).unwrap();
        assert!(read_leveldb(&directory.0).is_err());
        fs::write(directory.0.join("000006.ldb"), fixture_empty_table()).unwrap();
        let mut manifest = fixture_manifest();
        manifest.pop();
        fs::write(directory.0.join("MANIFEST-000001"), manifest).unwrap();
        assert!(read_leveldb(&directory.0).is_err());
    }

    #[test]
    fn manifest_level_move_keeps_the_new_live_reference() {
        let mut version = ManifestVersion::default();
        version.tables.insert((0, 5));
        // A file can move between levels without rewriting the table.
        apply_manifest_edit(&[7, 1, 5, 0, 0, 0, 6, 0, 5], &mut version).unwrap();
        assert_eq!(version.tables, HashSet::from([(1, 5)]));
        assert!(apply_manifest_edit(&[8], &mut version).is_err());
    }

    #[test]
    fn journal_uses_newest_sequence_and_respects_logout() {
        let mut store = Store::new();
        let mut journal = record(1, &batch(40, 1, b"\x01old"));
        journal.extend(record(1, &batch(42, 0, b"")));
        journal.extend(record(1, &batch(41, 1, b"\x01stale")));
        parse_log(&journal, &mut store).unwrap();
        let state = store.values().next().unwrap();
        assert_eq!(state.sequence, 42);
        assert!(state.value.is_none());
    }

    #[test]
    fn journal_accepts_fragmentation_and_ignores_partial_tail() {
        let data = batch(7, 1, b"\x01current");
        let split = 20;
        let first = record(2, &data[..split]);
        let mut journal = first;
        journal.resize(BLOCK_SIZE, 0);
        journal.extend(record(4, &data[split..]));
        journal.extend([0, 0, 0]);
        let mut store = Store::new();
        parse_log(&journal, &mut store).unwrap();
        assert_eq!(
            store.values().next().unwrap().value.as_deref(),
            Some(&b"\x01current"[..])
        );
    }

    #[test]
    fn damaged_journal_is_rejected() {
        let mut journal = record(1, &batch(1, 1, b"\x01session"));
        *journal.last_mut().unwrap() ^= 1;
        assert!(parse_log(&journal, &mut Store::new()).is_err());
        assert_eq!(crc32c(&[b"123456789"]), 0xe3069283);
    }

    #[test]
    fn snappy_literals_and_overlapping_copies_are_bounded() {
        assert_eq!(
            snappy_decode(&[6, 8, b'a', b'b', b'c', 10, 3, 0]).unwrap(),
            b"abcabc"
        );
        assert_eq!(snappy_decode(&[5, 0, b'a', 14, 1, 0]).unwrap(), b"aaaaa");
        assert!(snappy_decode(&[5, 14, 0, 0]).is_err());
        assert!(snappy_decode(&[1, 0, b'a', 0, b'b']).is_err());
    }

    #[test]
    fn chromium_value_supports_latin1_and_utf16() {
        assert_eq!(chromium_string(b"\x01UserInfo").unwrap(), "UserInfo");
        assert_eq!(
            chromium_string(&[0, 0x2d, 0x4e, 0x87, 0x65]).unwrap(),
            "中文"
        );
        assert!(chromium_string(&[0, 1]).is_err());
    }

    #[test]
    fn compacted_table_imports_and_journal_logout_wins() {
        let mut store = Store::new();
        parse_table(&fixture_table(20, 1, b"\x01active"), &mut store).unwrap();
        assert_eq!(
            store.values().next().unwrap().value.as_deref(),
            Some(&b"\x01active"[..])
        );
        parse_log(&record(1, &batch(21, 0, b"")), &mut store).unwrap();
        assert!(store.values().next().unwrap().value.is_none());
        parse_table(&fixture_table(20, 1, b"\x01active"), &mut store).unwrap();
        assert!(store.values().next().unwrap().value.is_none());
    }

    #[test]
    fn table_corruption_and_integer_overflow_are_rejected() {
        let mut table = fixture_table(2, 1, b"\x01data");
        table[0] ^= 1;
        assert!(parse_table(&table, &mut Store::new()).is_err());
        assert!(varint(&[255; 10], &mut 0).is_err());
        assert!(varint(&[128], &mut 0).is_err());
    }
}
