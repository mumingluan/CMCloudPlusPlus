//! Seekable random-access plaintext reader and loopback HTTP/1.1 Range server
//! for OpenList (`RCLONE\x00\x00` + XSalsa20-Poly1305) encrypted cloud files.

use crate::openlist_crypt_cipher::{
    OpenListCipher, BLOCK_DATA_SIZE, BLOCK_SIZE, FILE_HEADER_SIZE, FILE_NONCE_SIZE,
};
use axuanstor_sdk::cmcloud::PublicRanges;
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
    collections::HashMap,
    convert::Infallible,
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

type Body = UnsyncBoxBody<Bytes, io::Error>;

pub struct CryptRanges {
    ranges: PublicRanges,
    enc_size: u64,
    plain_size: u64,
    cipher: Arc<OpenListCipher>,
    initial_nonce: [u8; FILE_NONCE_SIZE],
    cancel: Arc<AtomicBool>,
    cached: Mutex<Option<(u64, Zeroizing<Vec<u8>>)>>,
}

impl CryptRanges {
    pub fn open(
        url: &str,
        enc_size: u64,
        cipher: Arc<OpenListCipher>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let plain_size = cipher.decrypted_size(enc_size)?;
        let ranges = PublicRanges::new(url, enc_size, cancel.clone())
            .map_err(|_| "无法建立加密文件分段读取连接，请检查网络或重试".to_string())?;
        let mut header = [0u8; 32];
        ranges
            .open(0, FILE_HEADER_SIZE)
            .map_err(|_| "无法读取加密文件头部".to_string())?
            .read_exact(&mut header)
            .map_err(|_| "读取加密文件头部不完整".to_string())?;
        let initial_nonce = OpenListCipher::parse_header(&header)?;
        Ok(Self {
            ranges,
            enc_size,
            plain_size,
            cipher,
            initial_nonce,
            cancel,
            cached: Mutex::new(None),
        })
    }

    pub fn plain_size(&self) -> u64 {
        self.plain_size
    }

    pub fn read_block(&self, block_index: u64) -> Result<Zeroizing<Vec<u8>>, String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("已取消读取".into());
        }
        if let Ok(guard) = self.cached.lock() {
            if let Some((idx, data)) = guard.as_ref() {
                if *idx == block_index {
                    return Ok(data.clone());
                }
            }
        }
        let enc_offset = FILE_HEADER_SIZE
            .checked_add(block_index.checked_mul(BLOCK_SIZE).ok_or("块偏移溢出")?)
            .ok_or("块偏移溢出")?;
        if enc_offset >= self.enc_size {
            return Err("读取位置超出加密文件末尾".into());
        }
        let enc_len = (self.enc_size - enc_offset).min(BLOCK_SIZE);
        let mut enc_buf = vec![0u8; enc_len as usize];
        self.ranges
            .open(enc_offset, enc_len)
            .map_err(|_| "读取加密数据分段失败".to_string())?
            .read_exact(&mut enc_buf)
            .map_err(|_| "加密数据分段不完整".to_string())?;
        let plain = self
            .cipher
            .open_block(block_index, &self.initial_nonce, &enc_buf)?;
        if let Ok(mut guard) = self.cached.lock() {
            *guard = Some((block_index, plain.clone()));
        }
        Ok(plain)
    }

    pub fn read_exact_at(&self, mut offset: u64, mut out: &mut [u8]) -> Result<(), String> {
        if offset.saturating_add(out.len() as u64) > self.plain_size {
            return Err("读取范围超出解密后文件大小".into());
        }
        while !out.is_empty() {
            if self.cancel.load(Ordering::Acquire) {
                return Err("读取已取消".into());
            }
            let block_idx = offset / BLOCK_DATA_SIZE;
            let in_block = (offset % BLOCK_DATA_SIZE) as usize;
            let block = self.read_block(block_idx)?;
            if in_block >= block.len() {
                return Err("加密块数据长度异常".into());
            }
            let avail = block.len() - in_block;
            let take = out.len().min(avail);
            out[..take].copy_from_slice(&block[in_block..in_block + take]);
            offset += take as u64;
            out = &mut out[take..];
        }
        Ok(())
    }
}

pub struct CryptPlayback {
    pub owner: String,
    pub handle: String,
    stop: CancellationToken,
    cancelled: Arc<AtomicBool>,
    pub url: String,
    pub mime: &'static str,
}

impl Drop for CryptPlayback {
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
    ranges: Arc<CryptRanges>,
}

pub fn mime_for(name: &str) -> Option<&'static str> {
    static FORMATS: OnceLock<HashMap<String, String>> = OnceLock::new();
    let extension = name
        .rsplit('/')
        .next()?
        .rsplit_once('.')?
        .1
        .to_ascii_lowercase();
    if let Some(m) = FORMATS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../src/mediaFormats.json"))
                .expect("valid bundled media types")
        })
        .get(&extension)
        .map(String::as_str)
    {
        return Some(m);
    }
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

pub async fn start_server(
    owner: String,
    handle: String,
    file_name: &str,
    mime: &'static str,
    ranges: Arc<CryptRanges>,
    cancelled: Arc<AtomicBool>,
) -> Result<(String, CryptPlayback), String> {
    let size = ranges.plain_size();
    if size == 0 {
        return Err("此文件为空，无法预览".into());
    }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "无法启动本机解密预览服务".to_string())?;
    let authority = listener
        .local_addr()
        .map_err(|_| "无法读取本机预览监听地址".to_string())?
        .to_string();
    let stream_id = uuid::Uuid::new_v4().to_string();
    let token = uuid::Uuid::new_v4();
    let encoded_name: String = url::form_urlencoded::byte_serialize(file_name.as_bytes())
        .map(|part| if part == "+" { "%20" } else { part })
        .collect();
    let req_path = format!("/{token}/{encoded_name}");
    let url = format!("http://{authority}{req_path}");
    let stop = CancellationToken::new();

    let media = Arc::new(Media {
        size,
        mime,
        authority,
        path: req_path,
        cancelled: cancelled.clone(),
        ranges,
    });
    tauri::async_runtime::spawn(serve(listener, media, stop.clone()));

    Ok((
        stream_id,
        CryptPlayback {
            owner,
            handle,
            stop,
            cancelled,
            url,
            mime,
        },
    ))
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
        return response.status(StatusCode::NOT_FOUND).body(empty()).unwrap();
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
        Some(value) => match value.to_str().ok().and_then(|v| requested_range(v, size)) {
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
        if self.cancelled.load(Ordering::Acquire) {
            return Err(io::Error::other("cancelled"));
        }
        let block_idx = offset / BLOCK_DATA_SIZE;
        let block = self
            .ranges
            .read_block(block_idx)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let start = (offset % BLOCK_DATA_SIZE) as usize;
        let length = (end - offset).min((block.len() - start) as u64) as usize;
        Ok(Bytes::copy_from_slice(&block[start..start + length]))
    }
}
