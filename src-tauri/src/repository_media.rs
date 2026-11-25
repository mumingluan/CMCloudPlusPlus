//! Loopback transport for seekable, authenticated repository media.
use super::{viewed_file_basis, error, logical_path, Arc, RepositoryManager};
use axuanstor_sdk::{
    cmcloud::{MobileCloud, PublicRanges},
    crypto::{BlobChunks, CHUNK_SIZE},
    repository::ObjectRef,
};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Empty, StreamBody};
use hyper::{
    body::{Bytes, Frame, Incoming},
    header,
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    convert::Infallible,
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

type Body = UnsyncBoxBody<Bytes, io::Error>;

pub(super) struct Playback {
    pub owner: String,
    pub handle: String,
    stop: CancellationToken,
    cancelled: Arc<AtomicBool>,
    url: String,
    mime: &'static str,
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.stop.cancel();
    }
}

struct Media {
    size: u64,
    mime: &'static str,
    authority: String,
    path: String,
    cancelled: Arc<AtomicBool>,
    chunks: BlobChunks,
    backend: Arc<MobileCloud>,
    object: ObjectRef,
    reader: Mutex<Reader>,
}

struct Reader {
    ranges: PublicRanges,
    cached: Option<(u64, Zeroizing<Vec<u8>>)>,
}

pub(super) fn mime_for(path: &str) -> Option<&'static str> {
    static FORMATS: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();
    let extension = path
        .rsplit('/')
        .next()?
        .rsplit_once('.')?
        .1
        .to_ascii_lowercase();
    FORMATS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../src/mediaFormats.json"))
                .expect("valid bundled media types")
        })
        .get(&extension)
        .map(String::as_str)
}

fn fetch_range(
    ranges: &PublicRanges,
    offset: u64,
    length: usize,
) -> axuanstor_sdk::Result<Vec<u8>> {
    let mut data = vec![0; length];
    ranges.open(offset, length as u64)?.read_exact(&mut data)?;
    Ok(data)
}

pub(super) async fn start(
    manager: Arc<RepositoryManager>,
    owner: String,
    handle: String,
    revision: String,
    path: String,
    mime: &'static str,
) -> Result<(String, String), String> {
    let session = manager.session(&handle, &owner)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let source = tokio::task::spawn_blocking(move || {
        let path = logical_path(&path).map_err(error)?;
        let mut state = session.state.lock().map_err(|_| "对象库正在操作")?;
        let view = viewed_file_basis(&session, &mut state, &revision, &path)?;
        let entry = view.resolve(&path).map_err(error)?;
        if entry.kind != "file" || entry.size == 0 {
            return Err("此文件没有可播放的内容".into());
        }
        let object = entry.object.clone().ok_or("文件引用不完整")?;
        let backend = session.backend.clone();
        let ranges = backend.open_ranges(&object, flag).map_err(error)?;
        let chunks = state
            .repo
            .open_file_chunks(&view, &path, |offset, length| {
                fetch_range(&ranges, offset, length)
            })
            .map_err(error)?;
        let filename = path.rsplit('/').next().ok_or("文件名无效")?;
        let filename: String = url::form_urlencoded::byte_serialize(filename.as_bytes())
            .map(|part| if part == "+" { "%20" } else { part })
            .collect();
        Ok::<_, String>((entry.size, object, backend, ranges, chunks, filename))
    })
    .await
    .map_err(|_| "流式预览准备已中断")??;

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "无法启动本机流式预览")?;
    let authority = listener
        .local_addr()
        .map_err(|_| "无法读取流式预览地址")?
        .to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let token = uuid::Uuid::new_v4();
    let (size, object, backend, ranges, chunks, filename) = source;
    let path = format!("/{token}/{filename}");
    let url = format!("http://{authority}{path}");
    let stop = CancellationToken::new();
    let media = Arc::new(Media {
        size,
        mime,
        authority,
        path,
        cancelled: cancelled.clone(),
        chunks,
        backend,
        object,
        reader: Mutex::new(Reader {
            ranges,
            cached: None,
        }),
    });
    {
        // Serialize registration with repository close, including while a grant was being fetched.
        let sessions = manager.sessions.lock().map_err(|_| "对象库会话不可用")?;
        if !sessions
            .get(&handle)
            .is_some_and(|session| session.scope == owner)
        {
            return Err("对象库已关闭，请重新打开".into());
        }
        let mut playbacks = manager.playbacks.lock().map_err(|_| "流式预览状态不可用")?;
        if playbacks.len() >= 8 {
            return Err("打开的流式预览过多，请关闭后重试".into());
        }
        playbacks.insert(
            id.clone(),
            Playback {
                owner,
                handle,
                stop: stop.clone(),
                cancelled,
                url: url.clone(),
                mime,
            },
        );
    }
    tauri::async_runtime::spawn(serve(listener, media, stop));
    Ok((id, url))
}

pub(super) fn release(manager: &RepositoryManager, id: &str) -> Result<bool, String> {
    Ok(manager
        .playbacks
        .lock()
        .map_err(|_| "流式预览状态不可用")?
        .remove(id)
        .is_some())
}

pub(super) fn external_source(
    manager: &RepositoryManager,
    owner: &str,
    id: &str,
) -> Result<(String, &'static str), String> {
    let playbacks = manager.playbacks.lock().map_err(|_| "流式预览状态不可用")?;
    let playback = playbacks
        .get(id)
        .filter(|playback| playback.owner == owner)
        .ok_or("流式预览已关闭，请重新打开")?;
    Ok((playback.url.clone(), playback.mime))
}

#[cfg(target_os = "windows")]
pub(super) fn open_windows_player(url: &str) -> Result<(), String> {
    use std::{path::Path, process::Command};
    let players = [
        "VideoLAN/VLC/vlc.exe",
        "MPC-BE x64/mpc-be64.exe",
        "MPC-HC/mpc-hc64.exe",
        "K-Lite Codec Pack/MPC-HC64/mpc-hc64.exe",
        "mpv/mpv.exe",
    ];
    for directory in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
    {
        for relative in players {
            let executable = Path::new(&directory).join(relative);
            if executable.is_file() && Command::new(executable).arg(url).spawn().is_ok() {
                return Ok(());
            }
        }
    }
    Err("未找到可启动的 VLC、MPC 或 mpv。请安装播放器，或下载后打开。".into())
}

async fn serve(listener: TcpListener, media: Arc<Media>, stop: CancellationToken) {
    let slots = Arc::new(Semaphore::new(8));
    loop {
        let stream = tokio::select! {
            _ = stop.cancelled() => return,
            result = listener.accept() => match result {
                Ok((stream, _)) => stream,
                Err(_) => return,
            },
        };
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            continue;
        };
        let media = media.clone();
        let stop = stop.clone();
        tauri::async_runtime::spawn(async move {
            let _slot = slot;
            let mut server = http1::Builder::new();
            server
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(5))
                .max_headers(32)
                .max_buf_size(16 * 1024);
            let connection = server.serve_connection(
                TokioIo::new(stream),
                service_fn(move |request| {
                    let response = respond(request, media.clone());
                    async move { Ok::<_, Infallible>(response) }
                }),
            );
            tokio::select! {
                _ = stop.cancelled() => (),
                _ = connection => (),
            }
        });
    }
}

/// Inclusive start, exclusive end. Media clients use a single range and may seek
/// from either end of the file.
fn requested_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let number = |s: &str| {
        s.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    if start.is_empty() {
        let suffix = number(end)?;
        return (suffix != 0 && size != 0).then_some((size.saturating_sub(suffix), size));
    }
    let start = number(start)?;
    let end = if end.is_empty() {
        size
    } else {
        number(end)?.saturating_add(1).min(size)
    };
    (start < end).then_some((start, end))
}

fn empty() -> Body {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed_unsync()
}

fn respond(request: Request<Incoming>, media: Arc<Media>) -> Response<Body> {
    let response = Response::builder()
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header("Referrer-Policy", "no-referrer");
    if media.cancelled.load(Ordering::Acquire)
        || request.uri().path() != media.path
        || request
            .headers()
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            != Some(media.authority.as_str())
    {
        return response
            .status(StatusCode::NOT_FOUND)
            .body(empty())
            .unwrap();
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return response
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(empty())
            .unwrap();
    }
    let size = media.size;
    let head = request.method() == Method::HEAD;
    let range = request.headers().get(header::RANGE).filter(|_| !head);
    let mut response = response.header(header::ACCEPT_RANGES, "bytes");
    let (start, end) = match range {
        None => (0, size),
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|value| requested_range(value, size))
        {
            Some((start, end)) => {
                response = response.status(StatusCode::PARTIAL_CONTENT).header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{}/{size}", end - 1),
                );
                (start, end)
            }
            None => {
                return response
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                    .body(empty())
                    .unwrap()
            }
        },
    };
    response = response
        .header(header::CONTENT_TYPE, media.mime)
        .header(header::CONTENT_LENGTH, end - start);
    let body = if head {
        empty()
    } else {
        StreamBody::new(futures_util::stream::try_unfold(
            (media, start),
            move |(media, offset)| async move {
                if offset == end {
                    return Ok(None);
                }
                let source = media.clone();
                let data = tokio::task::spawn_blocking(move || source.read_part(offset, end))
                    .await
                    .map_err(|_| io::Error::other("media read interrupted"))??;
                let next = offset + data.len() as u64;
                Ok(Some((Frame::data(data), (media, next))))
            },
        ))
        .boxed_unsync()
    };
    response.body(body).unwrap()
}

impl Media {
    fn read_part(&self, offset: u64, end: u64) -> io::Result<Bytes> {
        let failure = || io::Error::other("authenticated media range unavailable");
        let mut reader = self.reader.lock().map_err(|_| failure())?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(failure());
        }
        let index = offset / CHUNK_SIZE as u64;
        if reader.cached.as_ref().map(|chunk| chunk.0) != Some(index) {
            let fetch = |ranges: &PublicRanges| {
                self.chunks
                    .chunk(index, |offset, length| fetch_range(ranges, offset, length))
            };
            let data = fetch(&reader.ranges)
                .or_else(|reason| {
                    if !matches!(
                        reason,
                        axuanstor_sdk::Error::Incomplete | axuanstor_sdk::Error::Io(_)
                    ) || self.cancelled.load(Ordering::Acquire)
                    {
                        return Err(reason);
                    }
                    reader.ranges = self
                        .backend
                        .open_ranges(&self.object, self.cancelled.clone())?;
                    fetch(&reader.ranges)
                })
                .map_err(|_| failure())?;
            reader.cached = Some((index, data));
        }
        let (_, chunk) = reader.cached.as_ref().ok_or_else(failure)?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(failure());
        }
        let start = (offset % CHUNK_SIZE as u64) as usize;
        let length = (end - offset).min((chunk.len() - start) as u64) as usize;
        Ok(Bytes::copy_from_slice(&chunk[start..start + length]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_media_ranges_including_tail_and_large_seeks() {
        assert_eq!(requested_range("bytes=0-1", 123), Some((0, 2)));
        assert_eq!(requested_range("bytes=40-", 123), Some((40, 123)));
        assert_eq!(requested_range("bytes=-23", 123), Some((100, 123)));
        assert_eq!(
            requested_range("bytes=4294967296-4294967298", 1 << 33),
            Some((1 << 32, (1 << 32) + 3))
        );
        for invalid in [
            "bytes=123-",
            "bytes=2-1",
            "bytes=-0",
            "bytes=0-1,4-6",
            "bytes=+0-1",
        ] {
            assert_eq!(requested_range(invalid, 123), None);
        }
    }
}
