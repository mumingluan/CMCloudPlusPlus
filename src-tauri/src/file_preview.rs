use crate::{
    archive_format::{self, Entry, Format},
    file_client,
    openlist_crypt::{self, CryptManager},
    openlist_crypt_media::CryptRanges,
    repositories, AppState, CloudFile,
};
use axuanstor_sdk::cmcloud::{self, PublicRanges};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, State};
#[cfg(not(target_os = "android"))]
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "android")]
use {tauri::Manager, tauri_plugin_document_picker::DocumentPickerExt};

const TEXT_LIMIT: u64 = 512 << 10;
const TEXT_LIMIT_MESSAGE: &str = "文本超过 512 KB 预览上限，请下载后打开。";
const PDF_LIMIT: u64 = 64 << 20;
const OFFICE_LIMIT: u64 = 24 << 20;
const DIRECTORY_BYTES: u64 = 32 << 20;
const MEMBER_BYTES: u64 = 768 << 20;
const ARCHIVE_TTL: Duration = Duration::from_secs(20 * 60);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Source {
    Cloud { file: CloudFile },
    Repository { handle: String, revision: String, path: String },
    Crypt { handle: String, path: String },
}

pub(crate) trait ArchiveRangeSource: Send + Sync {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String>;
}

impl ArchiveRangeSource for PublicRanges {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut body = self
            .open(offset, buf.len() as u64)
            .map_err(|_| "exact archive range unavailable".to_string())?;
        body.read_exact(buf).map_err(|e| e.to_string())
    }
}

impl ArchiveRangeSource for CryptRanges {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        CryptRanges::read_exact_at(self, offset, buf)
    }
}

struct OpenArchive {
    owner: String,
    ranges: Arc<dyn ArchiveRangeSource>,
    format: Format,
    name: String,
    size: u64,
    entries: Vec<Entry>,
    cancel: Arc<AtomicBool>,
    opened: Instant,
}

pub(crate) struct ArchivePreviews {
    sessions: Mutex<HashMap<String, Arc<OpenArchive>>>,
    slot: Arc<tokio::sync::Semaphore>,
}

impl Default for ArchivePreviews {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            slot: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArchiveView {
    id: String,
    format: &'static str,
    entries: Vec<Entry>,
}

// ZIP and 7Z directory seeks and TAR/RAR header walks read independent bounded
// pages. A local byte budget also bounds formats that must scan sequentially.
struct RangeCursor {
    source: Arc<dyn ArchiveRangeSource>,
    cancel: Arc<AtomicBool>,
    size: u64,
    position: u64,
    buffer_start: u64,
    buffer: Vec<u8>,
    remaining: u64,
    deadline: Instant,
}

impl RangeCursor {
    fn new(source: Arc<dyn ArchiveRangeSource>, cancel: Arc<AtomicBool>, size: u64, budget: u64, timeout: Duration) -> Self {
        Self {
            source, cancel, size, position: 0, buffer_start: 0,
            buffer: Vec::new(), remaining: budget, deadline: Instant::now() + timeout,
        }
    }
}

impl Read for RangeCursor {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.position == self.size {
            return Ok(0);
        }
        if self.cancel.load(Ordering::Acquire) || Instant::now() > self.deadline {
            return Err(io::Error::other("archive read cancelled"));
        }
        if self.position < self.buffer_start || self.position >= self.buffer_start + self.buffer.len() as u64 {
            let available = self.size - self.position;
            let wanted = (output.len() as u64).clamp(4096, 256 << 10);
            let count = wanted.min(available).min(self.remaining);
            if count == 0 {
                return Err(io::Error::other("archive byte budget reached"));
            }
            self.buffer.resize(count as usize, 0);
            self.source
                .read_exact_at(self.position, &mut self.buffer)
                .map_err(io::Error::other)?;
            self.remaining -= count;
            self.buffer_start = self.position;
        }
        let start = (self.position - self.buffer_start) as usize;
        let len = output.len().min(self.buffer.len() - start);
        output[..len].copy_from_slice(&self.buffer[start..start + len]);
        self.position += len as u64;
        Ok(len)
    }
}

impl Seek for RangeCursor {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let offset = match position {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.size) + i128::from(n),
        };
        if offset < 0 || offset > i128::from(self.size) {
            return Err(io::Error::other("archive offset outside original object"));
        }
        self.position = offset as u64;
        Ok(self.position)
    }
}

fn decode_text(raw: &[u8]) -> Result<String, String> {
    let text = if raw.starts_with(&[0xff, 0xfe]) || raw.starts_with(&[0xfe, 0xff]) {
        let little = raw[0] == 0xff;
        let bytes = &raw[2..];
        if bytes.len() % 2 != 0 {
            return Err("文本编码不完整，请下载后打开。".into());
        }
        let words: Vec<_> = bytes.chunks_exact(2).map(|pair| {
            if little { u16::from_le_bytes([pair[0], pair[1]]) } else { u16::from_be_bytes([pair[0], pair[1]]) }
        }).collect();
        String::from_utf16(&words).map_err(|_| "文本编码不受支持，请下载后打开。")?
    } else {
        let bytes = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
        std::str::from_utf8(bytes).map_err(|_| "文本编码不受支持，请下载后打开。")?.to_owned()
    };
    if text.contains('\0') {
        return Err("文件包含二进制内容，请下载后打开。".into());
    }
    Ok(text)
}

async fn preview_owner(state: &AppState) -> String {
    match state.client.read().await.as_ref() {
        Some(cloud) => repositories::scope(&cloud.account_config().await),
        None => String::new(),
    }
}

async fn bounded_preview_bytes(
    state: State<'_, AppState>,
    manager: State<'_, Arc<repositories::RepositoryManager>>,
    crypt: State<'_, Arc<CryptManager>>,
    source: Source,
    limit: u64,
    limit_message: &'static str,
) -> Result<Vec<u8>, String> {
    let owner = preview_owner(&state).await;
    let manager = manager.inner().clone();
    let crypt = crypt.inner().clone();
    let crypt_client = match &source {
        Source::Crypt { handle, .. } => {
            let sess = crypt.session(handle, &owner)?;
            Some(file_client(&state, &sess.root_folder).await?)
        }
        _ => None,
    };
    let address = match &source {
        Source::Cloud { file } => {
            if file.is_dir || file.size > limit {
                return Err(limit_message.into());
            }
            Some(file_client(&state, file).await?.link(file).await?)
        }
        Source::Repository { .. } | Source::Crypt { .. } => None,
    };
    tokio::task::spawn_blocking(move || {
        let raw = match source {
            Source::Cloud { .. } => {
                let mut content = Vec::new();
                cmcloud::open_public_payload(address.as_deref().ok_or("预览源不可用")?, Arc::new(AtomicBool::new(false)))
                    .map_err(|_| "预览源不可用，请下载后打开。")?
                    .take(limit + 1).read_to_end(&mut content)
                    .map_err(|_| "预览文件读取失败，请下载后打开。")?;
                content
            }
            Source::Repository { handle, revision, path } => repositories::preview::preview_bytes(
                &manager, &owner, &handle, &revision, &path, limit, limit_message,
            )?,
            Source::Crypt { handle, path } => openlist_crypt::preview_bytes(
                &crypt,
                crypt_client.ok_or("加密预览客户端不可用")?,
                &owner,
                &handle,
                &path,
                limit,
                limit_message,
            )?,
        };
        if raw.len() as u64 > limit {
            return Err(limit_message.into());
        }
        Ok(raw)
    }).await.map_err(|_| "文件预览已中断")?
}

#[tauri::command]
pub(crate) async fn read_text_preview(
    state: State<'_, AppState>,
    manager: State<'_, Arc<repositories::RepositoryManager>>,
    crypt: State<'_, Arc<CryptManager>>,
    source: Source,
) -> Result<String, String> {
    decode_text(&bounded_preview_bytes(state, manager, crypt, source, TEXT_LIMIT, TEXT_LIMIT_MESSAGE).await?)
}

#[tauri::command]
pub(crate) async fn read_pdf_preview(
    state: State<'_, AppState>,
    manager: State<'_, Arc<repositories::RepositoryManager>>,
    crypt: State<'_, Arc<CryptManager>>,
    source: Source,
) -> Result<tauri::ipc::Response, String> {
    let bytes = bounded_preview_bytes(state, manager, crypt, source, PDF_LIMIT,
        "PDF 超过 64 MB 预览上限，请下载后打开。").await?;
    if !bytes[..bytes.len().min(1024)].windows(5).any(|sig| sig == b"%PDF-") {
        return Err("文件缺少有效的 PDF 标头，请下载后检查。".into());
    }
    Ok(tauri::ipc::Response::new(bytes))
}

#[derive(Serialize)]
pub(crate) struct OfficeParts {
    xml: BTreeMap<String, String>,
    images: BTreeMap<String, String>,
}

fn office_parts(bytes: Vec<u8>) -> Result<OfficeParts, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    const INVALID: &str = "无法读取 Office 文件，文件可能损坏或受到密码保护。";
    const BUDGET: &str = "Office 文档内容超过预览上限，请下载后打开。";
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| INVALID)?;
    if archive.len() > 4096 { return Err(BUDGET.into()); }
    let mut parts = OfficeParts { xml: BTreeMap::new(), images: BTreeMap::new() };
    let mut remaining = 32 << 20;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| INVALID)?;
        if file.is_dir() || file.enclosed_name().is_none() { continue; }
        let name = file.name().to_owned();
        if !["word/", "xl/", "ppt/"].iter().any(|prefix| name.starts_with(prefix)) { continue; }
        let extension = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        let mime = match extension.as_str() {
            "png" => Some("image/png"), "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"), "webp" => Some("image/webp"), _ => None,
        };
        let xml = extension == "xml" || extension == "rels";
        if !xml && (mime.is_none() || !name.contains("/media/")) { continue; }
        let limit = if xml { 8 << 20 } else { 4 << 20 };
        if file.size() > limit || file.size() > remaining { return Err(BUDGET.into()); }
        let expected = file.size();
        let mut data = Vec::new();
        file.by_ref().take(expected + 1).read_to_end(&mut data).map_err(|_| INVALID)?;
        if data.len() as u64 != expected { return Err(INVALID.into()); }
        remaining -= data.len() as u64;
        if xml {
            if parts.xml.insert(name, decode_text(&data).map_err(|_| INVALID)?).is_some() {
                return Err(INVALID.into());
            }
        } else if let Some(mime) = mime {
            if parts.images.insert(name, format!("data:{mime};base64,{}", STANDARD.encode(&data))).is_some() {
                return Err(INVALID.into());
            }
        }
    }
    if !["word/document.xml", "xl/workbook.xml", "ppt/presentation.xml"].iter().any(|key| parts.xml.contains_key(*key)) {
        return Err(INVALID.into());
    }
    Ok(parts)
}

#[tauri::command]
pub(crate) async fn read_office_preview(
    state: State<'_, AppState>,
    manager: State<'_, Arc<repositories::RepositoryManager>>,
    crypt: State<'_, Arc<CryptManager>>,
    source: Source,
) -> Result<OfficeParts, String> {
    let bytes = bounded_preview_bytes(state, manager, crypt, source, OFFICE_LIMIT,
        "Office 文件超过 24 MB 预览上限，请下载后打开。").await?;
    tokio::task::spawn_blocking(move || office_parts(bytes)).await.map_err(|_| "Office 预览已中断")?
}

#[tauri::command]
pub(crate) async fn open_archive_preview(
    state: State<'_, AppState>,
    manager: State<'_, Arc<repositories::RepositoryManager>>,
    crypt: State<'_, Arc<CryptManager>>,
    previews: State<'_, ArchivePreviews>,
    source: Source,
) -> Result<ArchiveView, String> {
    let _slot = previews.slot.clone().try_acquire_owned().map_err(|_| "正在处理另一个压缩包，请稍后重试。")?;
    let owner = preview_owner(&state).await;
    let crypt_client = match &source {
        Source::Crypt { handle, .. } => {
            let sess = crypt.session(handle, &owner)?;
            Some(file_client(&state, &sess.root_folder).await?)
        }
        _ => None,
    };
    let address = match &source {
        Source::Cloud { file } => {
            Format::from_name(&file.name)?;
            if file.is_dir || file.size == 0 {
                return Err("压缩包大小不可用，请刷新文件列表。".into());
            }
            Some(file_client(&state, file).await?.link(file).await?)
        }
        Source::Repository { .. } | Source::Crypt { .. } => None,
    };
    let manager = manager.inner().clone();
    let crypt = crypt.inner().clone();
    let session = tokio::task::spawn_blocking(move || -> Result<OpenArchive, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let (ranges, size, name): (Arc<dyn ArchiveRangeSource>, u64, String) = match source {
            Source::Cloud { file } => (
                Arc::new(
                    PublicRanges::new(address.as_deref().ok_or("下载地址不可用")?, file.size, cancel.clone())
                        .map_err(|_| "无法安全读取此压缩包，请下载后打开。")?,
                ),
                file.size,
                file.name,
            ),
            Source::Repository { handle, revision, path } => {
                let (r, sz, nm) = repositories::preview::archive_ranges(
                    &manager, &owner, &handle, &revision, &path, cancel.clone(),
                )?;
                (Arc::new(r), sz, nm)
            }
            Source::Crypt { handle, path } => {
                let (r, sz, nm) = openlist_crypt::archive_ranges(
                    &crypt,
                    crypt_client.ok_or("加密预览客户端不可用")?,
                    &owner,
                    &handle,
                    &path,
                    cancel.clone(),
                )?;
                (r, sz, nm)
            }
        };
        let format = Format::from_name(&name)?;
        let reader = RangeCursor::new(ranges.clone(), cancel.clone(), size, DIRECTORY_BYTES, Duration::from_secs(90));
        let entries = archive_format::directory(format, reader, &name)?;
        Ok(OpenArchive { owner, ranges, format, name, size, entries, cancel, opened: Instant::now() })
    }).await.map_err(|_| "压缩包预览已中断")??;
    let id = uuid::Uuid::new_v4().to_string();
    let view = ArchiveView { id: id.clone(), format: session.format.label(), entries: session.entries.clone() };
    let mut map = previews.sessions.lock().map_err(|_| "压缩包预览状态不可用")?;
    map.retain(|_, item| item.opened.elapsed() < ARCHIVE_TTL);
    if map.len() >= 4 {
        if let Some(oldest) = map.iter().min_by_key(|(_, item)| item.opened).map(|(id, _)| id.clone()) {
            if let Some(old) = map.remove(&oldest) {
                old.cancel.store(true, Ordering::Release);
            }
        }
    }
    map.insert(id, Arc::new(session));
    Ok(view)
}

#[tauri::command]
pub(crate) fn close_archive_preview(previews: State<'_, ArchivePreviews>, id: String) {
    if let Ok(mut map) = previews.sessions.lock() {
        if let Some(session) = map.remove(&id) {
            session.cancel.store(true, Ordering::Release);
        }
    }
}

#[tauri::command]
pub(crate) async fn save_archive_member(
    app: AppHandle, state: State<'_, AppState>, previews: State<'_, ArchivePreviews>, id: String, member: usize,
) -> Result<bool, String> {
    let _slot = previews.slot.clone().try_acquire_owned().map_err(|_| "正在处理另一个压缩包，请稍后重试。")?;
    let owner = preview_owner(&state).await;
    let session = previews.sessions.lock().map_err(|_| "压缩包预览状态不可用")?
        .get(&id).cloned().ok_or("请重新打开压缩包预览。")?;
    if owner != session.owner || session.opened.elapsed() >= ARCHIVE_TTL {
        return Err("预览已过期，请重新打开压缩包。".into());
    }
    let entry = session.entries.iter().find(|entry| entry.id == member && entry.downloadable)
        .cloned().ok_or("请选择可下载的成员。")?;
    let original = entry.path.rsplit('/').next().ok_or("成员文件名无效")?;
    let name = crate::transfers::sanitize_filename(original);
    crate::transfers::validate_filename(&name)?;
    #[cfg(not(target_os = "android"))]
    let directory = {
        let app = app.clone();
        let picked = tokio::task::spawn_blocking(move || {
            app.dialog().file().set_title("选择保存文件夹").blocking_pick_folder()
        }).await.map_err(|_| "文件夹选择已中断")?;
        let Some(picked) = picked else { return Ok(false); };
        picked.into_path().map_err(|_| "保存位置无效")?
    };
    #[cfg(target_os = "android")]
    let temporary = {
        let root = app.path().app_data_dir().map_err(|_| "下载目录不可用")?.join("Downloads/AxuanStor");
        std::fs::create_dir_all(&root).map_err(|_| "下载目录不可用")?;
        tempfile::tempdir_in(root).map_err(|_| "下载暂存目录不可用")?
    };
    #[cfg(target_os = "android")]
    let directory = temporary.path().to_owned();
    let path = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let output = directory.join(&name);
        let mut stage = tempfile::NamedTempFile::new_in(&directory).map_err(|_| "无法写入保存目录")?;
        let reader = RangeCursor::new(session.ranges.clone(), session.cancel.clone(), session.size, MEMBER_BYTES, Duration::from_secs(300));
        archive_format::extract(session.format, reader, &entry, stage.as_file_mut())?;
        if session.cancel.load(Ordering::Acquire) {
            return Err("压缩包预览已关闭。".into());
        }
        stage.as_file().sync_all().map_err(|_| "无法保存成员文件")?;
        stage.persist_noclobber(&output).map_err(|_| "保存失败，请检查同名文件或剩余空间。")?;
        Ok(output)
    }).await.map_err(|_| "成员下载已中断")??;
    #[cfg(target_os = "android")]
    { app.document_picker().save_shared_download(path.to_str().ok_or("下载路径无效")?).await }
    #[cfg(not(target_os = "android"))]
    { let _ = path; Ok(true) }
}

fn archive_selection(entries: &[Entry], paths: &[String]) -> Result<Vec<Entry>, String> {
    const LIMIT: &str = "一次最多可下载 1,024 个成员或 1 GB，请缩小选择范围。";
    if paths.is_empty() || paths.len() > 1024 { return Err(LIMIT.into()); }
    let selections: HashSet<_> = paths.iter().map(String::as_str).collect();
    let mut matched = HashSet::new();
    let mut names = HashMap::new();
    let mut selected = Vec::new();
    let mut size = 0u64;
    for entry in entries {
        let mut included = false;
        for part in std::iter::once(entry.path.as_str())
            .chain(entry.path.match_indices('/').map(|(n, _)| &entry.path[..n])) {
            if selections.contains(part) {
                included = true;
                matched.insert(part);
            }
        }
        if !included { continue; }
        if !entry.directory && !entry.downloadable {
            return Err("选择中有无法在线提取的成员，请下载原压缩包。".into());
        }
        if selected.len() >= 1024 { return Err(LIMIT.into()); }
        if !entry.directory {
            size = size.checked_add(entry.size.unwrap_or(0))
                .filter(|n| *n <= 1 << 30).ok_or(LIMIT)?;
        }
        for component in entry.path.split('/') {
            crate::transfers::validate_filename(component)
                .map_err(|_| "选择中有不适合保存的文件名，请下载原压缩包。")?;
        }
        let key = entry.path.to_lowercase();
        if let Some(directory) = names.insert(key, entry.directory) {
            if directory && entry.directory { continue; }
            return Err("选择中有同名成员，请单独下载。".into());
        }
        selected.push(entry.clone());
    }
    if selections != matched || selected.is_empty() {
        return Err("请选择压缩包里存在的文件或文件夹。".into());
    }
    for entry in &selected {
        if entry.path.match_indices('/').any(|(n, _)| names.get(&entry.path[..n].to_lowercase()) == Some(&false)) {
            return Err("选择中的文件与文件夹重名，请缩小选择范围。".into());
        }
    }
    Ok(selected)
}

struct BoundedExport<'a, W> {
    writer: &'a mut W,
    remaining: &'a mut u64,
    exceeded: bool,
}

impl<W: Write> Write for BoundedExport<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > *self.remaining {
            self.exceeded = true;
            return Err(io::Error::other("archive export limit"));
        }
        let written = self.writer.write(bytes)?;
        *self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> { self.writer.flush() }
}

#[tauri::command]
pub(crate) async fn save_archive_selection(
    app: AppHandle, state: State<'_, AppState>, previews: State<'_, ArchivePreviews>, id: String,
    paths: Vec<String>,
) -> Result<bool, String> {
    let _slot = previews.slot.clone().try_acquire_owned().map_err(|_| "正在处理另一个压缩包，请稍后重试。")?;
    let owner = preview_owner(&state).await;
    let session = previews.sessions.lock().map_err(|_| "压缩包预览状态不可用")?
        .get(&id).cloned().ok_or("请重新打开压缩包预览。")?;
    if owner != session.owner || session.opened.elapsed() >= ARCHIVE_TTL {
        return Err("预览已过期，请重新打开压缩包。".into());
    }
    let entries = archive_selection(&session.entries, &paths)?;
    let title: String = if paths.len() == 1 {
        paths[0].rsplit('/').next().unwrap_or("压缩包文件").chars().take(65).collect()
    } else {
        session.name.rsplit_once('.').map_or(session.name.as_str(), |(stem, _)| stem)
            .chars().take(65).collect()
    };
    let name = format!("{}{}.zip", crate::transfers::sanitize_filename(&title),
                       if paths.len() > 1 { "-选中项" } else { "" });
    #[cfg(not(target_os = "android"))]
    let directory = {
        let app = app.clone();
        let picked = tokio::task::spawn_blocking(move || {
            app.dialog().file().set_title("选择保存文件夹").blocking_pick_folder()
        }).await.map_err(|_| "文件夹选择已中断")?;
        let Some(picked) = picked else { return Ok(false); };
        picked.into_path().map_err(|_| "保存位置无效")?
    };
    #[cfg(target_os = "android")]
    let temporary = {
        let root = app.path().app_data_dir().map_err(|_| "下载目录不可用")?.join("Downloads/AxuanStor");
        std::fs::create_dir_all(&root).map_err(|_| "下载目录不可用")?;
        tempfile::tempdir_in(root).map_err(|_| "下载暂存目录不可用")?
    };
    #[cfg(target_os = "android")]
    let directory = temporary.path().to_owned();
    let path = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let output = directory.join(&name);
        if output.exists() { return Err("保存目录已有同名文件，请选择其他目录。".into()); }
        let mut stage = tempfile::NamedTempFile::new_in(&directory).map_err(|_| "无法写入保存目录")?;
        let mut writer = zip::ZipWriter::new(stage.as_file_mut());
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut remaining = 1 << 30;
        let mut zip_source = if matches!(session.format, Format::Zip) {
            let reader = RangeCursor::new(session.ranges.clone(), session.cancel.clone(),
                session.size, 1536 << 20, deadline.saturating_duration_since(Instant::now()));
            Some(zip::ZipArchive::new(reader).map_err(|_| "无法读取 ZIP 目录，请下载后打开。")?)
        } else { None };
        for entry in entries {
            if session.cancel.load(Ordering::Acquire) { return Err("压缩包预览已关闭。".into()); }
            if Instant::now() >= deadline { return Err("批量提取超时，请缩小选择范围。".into()); }
            if entry.directory {
                writer.add_directory(format!("{}/", entry.path), options).map_err(|_| "无法保存文件夹")?;
            } else {
                writer.start_file(&entry.path, options).map_err(|_| "无法创建成员文件")?;
                let mut output = BoundedExport { writer: &mut writer, remaining: &mut remaining, exceeded: false };
                let result = if let Some(zip) = zip_source.as_mut() {
                    archive_format::extract_zip_member(zip, &entry, &mut output)
                } else {
                    let reader = RangeCursor::new(session.ranges.clone(), session.cancel.clone(), session.size,
                        MEMBER_BYTES, deadline.saturating_duration_since(Instant::now()));
                    archive_format::extract(session.format, reader, &entry, &mut output)
                };
                if output.exceeded { return Err("选中内容超过 1 GB，请缩小选择范围。".into()); }
                result?;
            }
        }
        writer.finish().map_err(|_| "无法完成批量下载")?;
        if session.cancel.load(Ordering::Acquire) { return Err("压缩包预览已关闭。".into()); }
        stage.as_file().sync_all().map_err(|_| "无法保存批量下载")?;
        stage.persist_noclobber(&output).map_err(|_| "保存失败，请检查同名文件或剩余空间。")?;
        Ok(output)
    }).await.map_err(|_| "批量下载已中断")??;
    #[cfg(target_os = "android")]
    { app.document_picker().save_shared_download(path.to_str().ok_or("下载路径无效")?).await }
    #[cfg(not(target_os = "android"))]
    { let _ = path; Ok(true) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: usize, path: &str) -> Entry {
        Entry { id, path: path.into(), size: None, directory: false, downloadable: true }
    }

    #[test]
    fn archive_folder_and_individual_choices_share_one_member_plan() {
        let entries = vec![file(0, "manual/setup.txt"), file(1, "manual/faq.txt"), file(2, "readme.txt")];
        let wanted = vec!["manual".into(), "manual/faq.txt".into(), "readme.txt".into()];
        let selected = archive_selection(&entries, &wanted).unwrap();
        assert_eq!(selected.iter().map(|entry| entry.id).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(archive_selection(&entries, &["missing".into()]).is_err());
        assert!(archive_selection(&[file(0, "../escape")], &["../escape".into()]).is_err());
    }

    #[test]
    fn office_preview_extracts_only_bounded_document_parts() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("word/document.xml", "<document><body>test</body></document>"),
            ("word/vbaProject.bin", "ignored"),
            ("outside.xml", "<external/>"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        let parts = office_parts(zip.finish().unwrap().into_inner()).unwrap();
        assert_eq!(parts.xml.len(), 1);
        assert!(parts.xml["word/document.xml"].contains("test"));
        assert!(parts.images.is_empty());
    }

    #[test]
    fn unknown_member_size_still_obeys_batch_output_limit() {
        let mut bytes = Vec::new();
        let mut remaining = 3;
        let mut output = BoundedExport { writer: &mut bytes, remaining: &mut remaining, exceeded: false };
        assert!(output.write_all(b"four").is_err());
        assert!(output.exceeded);
        assert!(bytes.is_empty());
    }
}
