use serde::Serialize;
use std::io::{self, Read, Seek, SeekFrom, Write};

const MAX_ENTRIES: usize = 50_000;
const MAX_STREAM_SCAN: u64 = 256 << 20;
pub(crate) const MAX_MEMBER: u64 = 512 << 20;
const BUFFERED_MEMBER: u64 = 128 << 20;

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Zip,
    Tar,
    TarGzip,
    Gzip,
    SevenZip,
    Rar,
}

impl Format {
    pub(crate) fn from_name(name: &str) -> Result<Self, String> {
        let name = name.to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            return Ok(Self::TarGzip);
        }
        match name.rsplit('.').next().unwrap_or("") {
            "zip" => Ok(Self::Zip),
            "tar" => Ok(Self::Tar),
            "gz" => Ok(Self::Gzip),
            "7z" => Ok(Self::SevenZip),
            "rar" => Ok(Self::Rar),
            _ => Err("此压缩格式请下载后打开。".into()),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Tar => "TAR",
            Self::TarGzip => "TAR.GZ",
            Self::Gzip => "GZIP",
            Self::SevenZip => "7Z",
            Self::Rar => "RAR",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub id: usize,
    pub path: String,
    pub size: Option<u64>,
    pub directory: bool,
    pub downloadable: bool,
}

fn member_path(raw: &str) -> Option<String> {
    let raw = raw.replace('\\', "/");
    let path = raw.trim_start_matches("./").trim_end_matches('/');
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control)
        || path.contains(':') || path.split('/').count() > 64
        || path.split('/').any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(path.to_owned())
}

fn push(entries: &mut Vec<Entry>, id: usize, raw: &str, size: Option<u64>, dir: bool, readable: bool) -> Result<(), String> {
    if id >= MAX_ENTRIES {
        return Err("压缩包目录超过 50,000 项，请下载后打开。".into());
    }
    if let Some(path) = member_path(raw) {
        entries.push(Entry {
            id, path, size, directory: dir,
            downloadable: readable && !dir && size.is_none_or(|n| n <= MAX_MEMBER),
        });
    }
    Ok(())
}

struct ScanLimit<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for ScanLimit<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 && !buf.is_empty() {
            return Err(io::Error::other("archive scan budget reached"));
        }
        let take = (buf.len() as u64).min(self.remaining) as usize;
        let n = self.inner.read(&mut buf[..take])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

fn archive_error() -> String {
    "无法读取压缩包。文件可能损坏、加密，或超出在线扫描上限。请下载后打开。".into()
}

fn seven_header<R: Read + Seek>(source: &mut R) -> Result<(), String> {
    let size = source.seek(SeekFrom::End(0)).map_err(|_| archive_error())?;
    source.rewind().map_err(|_| archive_error())?;
    let mut header = [0u8; 32];
    source.read_exact(&mut header).map_err(|_| archive_error())?;
    let offset = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let length = u64::from_le_bytes(header[20..28].try_into().unwrap());
    if header[..6] != *b"7z\xbc\xaf\x27\x1c" || length > 16 << 20
        || offset.checked_add(length).and_then(|end| end.checked_add(32)).is_none_or(|end| end > size)
    {
        return Err(archive_error());
    }
    source.rewind().map_err(|_| archive_error())
}

fn rar_options() -> rars::ArchiveReadOptions<'static> {
    rars::ArchiveReadOptions::default()
        .with_max_header_count(MAX_ENTRIES as u64 + 64)
        .with_max_header_bytes(16 << 20)
        .with_rar50_dictionary_size_limit(64 << 20)
        .with_max_member_output_bytes(BUFFERED_MEMBER)
        .with_max_total_output_bytes(MAX_STREAM_SCAN)
}

pub(crate) fn directory<R: Read + Seek + Send + 'static>(
    format: Format, mut source: R, name: &str,
) -> Result<Vec<Entry>, String> {
    let mut rows = Vec::new();
    match format {
        Format::Zip => {
            let zip = zip::ZipArchive::new(source).map_err(|_| archive_error())?;
            for (id, name) in zip.file_names().enumerate() {
                let dir = name.ends_with(['/', '\\']);
                push(&mut rows, id, name, None, dir, true)?;
            }
        }
        Format::Tar => {
            let mut tar = tar::Archive::new(source);
            tar_directory(tar.entries_with_seek().map_err(|_| archive_error())?, &mut rows)?;
        }
        Format::TarGzip => {
            let decoder = flate2::read::MultiGzDecoder::new(source);
            let mut tar = tar::Archive::new(ScanLimit { inner: decoder, remaining: MAX_STREAM_SCAN });
            tar_directory(tar.entries().map_err(|_| archive_error())?, &mut rows)?;
        }
        Format::Gzip => {
            let mut magic = [0u8; 3];
            source.read_exact(&mut magic).map_err(|_| archive_error())?;
            if magic != [0x1f, 0x8b, 8] {
                return Err(archive_error());
            }
            let uncompressed = name.get(..name.len().saturating_sub(3)).unwrap_or("解压文件");
            push(&mut rows, 0, uncompressed, None, false, true)?;
        }
        Format::SevenZip => {
            seven_header(&mut source)?;
            let seven = sevenz_rust2::ArchiveReader::new(source, sevenz_rust2::Password::empty())
                .map_err(|_| archive_error())?;
            for (id, file) in seven.archive().files.iter().enumerate() {
                if !file.is_anti_item {
                    push(&mut rows, id, &file.name, Some(file.size), file.is_directory, file.size <= BUFFERED_MEMBER)?;
                }
            }
        }
        Format::Rar => {
            let rar = rars::ArchiveReader::read_reader_with_options(source, rar_options())
                .map_err(|_| archive_error())?;
            for (id, file) in rar.members().enumerate() {
                let info = file.meta;
                let readable = !(info.is_redirection || info.is_encrypted || info.is_split_before || info.is_split_after)
                    && info.unpacked_size <= BUFFERED_MEMBER;
                push(&mut rows, id, &info.name_lossy(), Some(info.unpacked_size), info.is_directory, readable)?;
            }
        }
    }
    Ok(rows)
}

fn tar_directory<R: Read>(entries: tar::Entries<'_, R>, rows: &mut Vec<Entry>) -> Result<(), String> {
    for (id, entry) in entries.enumerate() {
        let entry = entry.map_err(|_| archive_error())?;
        let kind = entry.header().entry_type();
        if kind.is_file() || kind.is_dir() {
            push(rows, id, &String::from_utf8_lossy(&entry.path_bytes()), Some(entry.size()), kind.is_dir(), true)?;
        }
    }
    Ok(())
}

fn copy_member(mut source: impl Read, out: &mut dyn Write, size: Option<u64>) -> Result<u64, String> {
    let limit = size.unwrap_or(MAX_MEMBER).min(MAX_MEMBER);
    let bytes = io::copy(&mut source.by_ref().take(limit + 1), out)
        .map_err(|_| "成员解压失败，请下载压缩包后打开。".to_string())?;
    if bytes > limit || size.is_some_and(|expected| expected != bytes) {
        return Err("解压后的文件大小无效或超过单文件上限。".into());
    }
    Ok(bytes)
}

pub(crate) fn extract<R: Read + Seek + Send + 'static>(
    format: Format, mut source: R, selected: &Entry, out: &mut dyn Write,
) -> Result<u64, String> {
    if !selected.downloadable || selected.directory {
        return Err("此成员请下载压缩包后解压。".into());
    }
    match format {
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(source).map_err(|_| archive_error())?;
            extract_zip_member(&mut zip, selected, out)
        }
        Format::Tar => {
            let mut tar = tar::Archive::new(source);
            tar_extract(tar.entries_with_seek().map_err(|_| archive_error())?, selected, out)
        }
        Format::TarGzip => {
            let decoder = flate2::read::MultiGzDecoder::new(source);
            let mut tar = tar::Archive::new(ScanLimit { inner: decoder, remaining: MAX_STREAM_SCAN + MAX_MEMBER });
            tar_extract(tar.entries().map_err(|_| archive_error())?, selected, out)
        }
        Format::Gzip => copy_member(flate2::read::MultiGzDecoder::new(source), out, None),
        Format::SevenZip => {
            seven_header(&mut source)?;
            let mut seven = sevenz_rust2::ArchiveReader::new(source, sevenz_rust2::Password::empty())
                .map_err(|_| archive_error())?;
            seven.set_thread_count(1);
            let archive = seven.archive();
            let item = archive.files.get(selected.id).ok_or_else(archive_error)?;
            if item.is_anti_item || item.is_directory || item.size > BUFFERED_MEMBER
                || member_path(&item.name).as_deref() != Some(&selected.path)
                || selected.size != Some(item.size) {
                return Err(archive_error());
            }
            // Solid predecessors may also have to be decoded. Bound their
            // cumulative output before asking the decoder for this member.
            if archive.is_solid && archive.files[..=selected.id].iter()
                .try_fold(0u64, |total, file| total.checked_add(file.size))
                .is_none_or(|total| total > BUFFERED_MEMBER) {
                return Err("固实 7Z 的解压范围过大，请下载压缩包后打开。".into());
            }
            if archive.files.iter().filter(|file| file.name == item.name).count() != 1 {
                return Err("压缩包存在同名成员，请下载后解压。".into());
            }
            let name = item.name.clone();
            let data = seven.read_file(&name).map_err(|_| archive_error())?;
            copy_member(data.as_slice(), out, selected.size)
        }
        Format::Rar => {
            let options = rar_options();
            let rar = rars::ArchiveReader::read_reader_with_options(source, options)
                .map_err(|_| archive_error())?;
            let item = rar.members().nth(selected.id).ok_or_else(archive_error)?.meta;
            if item.is_directory || item.is_redirection || item.is_encrypted || item.is_split_before || item.is_split_after
                || item.unpacked_size > BUFFERED_MEMBER || selected.size != Some(item.unpacked_size)
                || member_path(&item.name_lossy()).as_deref() != Some(&selected.path) {
                return Err(archive_error());
            }
            let data = rar.read_member_at_with_options(selected.id, options).map_err(|_| archive_error())?
                .ok_or_else(archive_error)?;
            copy_member(data.as_slice(), out, selected.size)
        }
    }
}

pub(crate) fn extract_zip_member<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>, selected: &Entry, out: &mut dyn Write,
) -> Result<u64, String> {
    let entry = zip.by_index(selected.id).map_err(|_| archive_error())?;
    let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
    if !entry.is_file() || mode != 0 && mode != 0o100000
        || member_path(entry.name()).as_deref() != Some(&selected.path)
        || entry.size() > MAX_MEMBER {
        return Err("此成员无法安全保存，请下载压缩包后解压。".into());
    }
    let size = entry.size();
    copy_member(entry, out, Some(size))
}

fn tar_extract<R: Read>(mut entries: tar::Entries<'_, R>, selected: &Entry, out: &mut dyn Write) -> Result<u64, String> {
    let item = entries.nth(selected.id).ok_or_else(archive_error)?.map_err(|_| archive_error())?;
    if !item.header().entry_type().is_file() || Some(item.size()) != selected.size
        || member_path(&String::from_utf8_lossy(&item.path_bytes())).as_deref() != Some(&selected.path) {
        return Err(archive_error());
    }
    copy_member(item, out, selected.size)
}
