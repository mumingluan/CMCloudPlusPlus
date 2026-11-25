//! Streaming, resumable, zero-spool upload encryption and authenticated block-aligned
//! download decryption for OpenList (`RCLONE\x00\x00` + XSalsa20-Poly1305) transfers.

use super::*;
use crate::openlist_crypt::CryptManager;
use crate::openlist_crypt_cipher::{
    CryptFolderOptions, OpenListCipher, BLOCK_DATA_SIZE, BLOCK_HEADER_SIZE, BLOCK_SIZE,
    FILE_HEADER_SIZE, FILE_NONCE_SIZE,
};
use hyper::body::Bytes;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CryptTransfer {
    pub kind: String,
    pub format: u32,
    pub owner: String,
    pub root_folder_id: String,
    pub target: CloudFile,
    pub encrypted_name: String,
    pub plain_name: String,
    pub plain_size: u64,
    pub enc_size: u64,
    pub options: CryptFolderOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_nonce_hex: Option<String>,
}

impl CryptTransfer {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.kind != "openlist_crypt"
            || self.format != 1
            || self.owner.len() != 64
            || self.root_folder_id.is_empty()
            || self.encrypted_name.is_empty()
            || self.plain_name.is_empty()
        {
            return Err("加密文件夹传输任务元数据无效".into());
        }
        let _ = self.options.normalized()?;
        if self.enc_size < FILE_HEADER_SIZE {
            return Err("加密文件长度元数据无效".into());
        }
        if let Some(hex_nonce) = &self.initial_nonce_hex {
            if hex_nonce.len() != FILE_NONCE_SIZE * 2 || hex::decode(hex_nonce).is_err() {
                return Err("加密上传初始随机数无效".into());
            }
        }
        Ok(())
    }

    pub(crate) fn queue_size_name(&self, spec: &TransferSpec) -> Result<(String, u64), String> {
        self.validate()?;
        if spec.remote.id != format!("openlist_crypt:{}", self.root_folder_id) {
            return Err("加密文件夹传输目标不一致".into());
        }
        if spec.direction == TransferDirection::Upload && self.initial_nonce_hex.is_none() {
            return Err("加密上传缺少随机数种子".into());
        }
        Ok((self.plain_name.clone(), self.plain_size))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum SpecialTransfer {
    OpenListCrypt(CryptTransfer),
    Repository(RepositoryTransfer),
}

impl SpecialTransfer {
    pub(crate) fn preview_path(&self) -> Option<&str> {
        match self {
            Self::OpenListCrypt(_) => None,
            Self::Repository(r) => r.preview_path(),
        }
    }

    pub(crate) fn queue_size_name(&self, spec: &TransferSpec) -> Result<(String, u64), String> {
        match self {
            Self::OpenListCrypt(c) => c.queue_size_name(spec),
            Self::Repository(r) => r.queue_size_name(spec),
        }
    }
}

impl TransferManager {
    pub(super) async fn run_crypt_transfer(
        self: &Arc<Self>,
        job: &TransferJob,
        client: &CloudClient,
        control: &Arc<Control>,
        intent: CryptTransfer,
    ) -> Result<(), String> {
        intent.validate()?;
        let cfg = client.account_config().await;
        let current_scope = crate::repositories::scope(&cfg);
        if current_scope != intent.owner {
            return Err("账号或云空间已切换，请切换回原账号继续此加密传输".into());
        }
        let crypt_manager = self.app.state::<Arc<CryptManager>>().inner().clone();
        let cipher = crypt_manager
            .cipher_for_transfer(
                &self.store,
                &self.app,
                &intent.owner,
                &intent.root_folder_id,
                &intent.options,
            )
            .await?;

        match job.direction {
            TransferDirection::Upload => {
                self.run_crypt_upload(job, client, control, &intent, cipher)
                    .await
            }
            TransferDirection::Download => {
                self.run_crypt_download(job, client, control, &intent, cipher)
                    .await
            }
        }
    }

    async fn run_crypt_upload(
        &self,
        job: &TransferJob,
        client: &CloudClient,
        control: &Arc<Control>,
        intent: &CryptTransfer,
        cipher: Arc<OpenListCipher>,
    ) -> Result<(), String> {
        let nonce_vec = hex::decode(
            intent
                .initial_nonce_hex
                .as_deref()
                .ok_or("缺少上传随机数")?,
        )
        .map_err(|_| "上传随机数损坏")?;
        let mut initial_nonce = [0u8; FILE_NONCE_SIZE];
        initial_nonce.copy_from_slice(&nonce_vec);

        let mut source = self.open_source(&job.local_path).await?;
        let metadata = source.metadata().await.map_err(|e| e.to_string())?;
        let plain_size = metadata.len();
        let modified = metadata_stamp(&metadata);
        if plain_size != intent.plain_size || cipher.encrypted_size(plain_size) != intent.enc_size {
            return Err("本地文件大小已变化，请重新添加加密上传任务".into());
        }
        let enc_size = intent.enc_size;
        let header = OpenListCipher::make_header(&initial_nonce);

        // Pass 1: Compute deterministic encrypted stream SHA-256 without writing any temp file.
        let enc_hash = {
            let mut hasher = Sha256::new();
            hasher.update(header);
            let mut buf = vec![0u8; BLOCK_DATA_SIZE as usize];
            let mut plain_read = 0u64;
            let mut block_idx = 0u64;
            let mut meter = Meter::new(0);
            while plain_read < plain_size {
                control.check()?;
                let want = (plain_size - plain_read).min(BLOCK_DATA_SIZE) as usize;
                source
                    .read_exact(&mut buf[..want])
                    .await
                    .map_err(|e| e.to_string())?;
                let sealed = cipher.seal_block(block_idx, &initial_nonce, &buf[..want])?;
                hasher.update(&sealed);
                plain_read += want as u64;
                block_idx += 1;
                if meter.ready() {
                    self.progress(
                        job,
                        plain_read,
                        plain_size,
                        meter.sample(plain_read),
                        "encrypting",
                    );
                }
            }
            hex::encode(hasher.finalize()).to_uppercase()
        };
        control.check()?;
        let after_hash = source.metadata().await.map_err(|e| e.to_string())?;
        if after_hash.len() != plain_size || metadata_stamp(&after_hash) != modified {
            return Err("加密校验期间本地文件发生变化，请重新添加任务".into());
        }

        let previous: Option<UploadJournal> = self
            .journal(&job.id)?
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .filter(|v| v["plan"]["legacy"].as_bool() != Some(true))
            .and_then(|v| serde_json::from_value(v).ok());

        let mut first_part = None;
        let mut journal = match previous {
            Some(mut j) if j.source_size == enc_size && j.source_modified == modified => {
                if !j.plan.content_hash.is_empty() && j.plan.content_hash != enc_hash {
                    return Err("加密内容校验不一致，请重新添加上传任务".into());
                }
                j.plan.content_hash = enc_hash.clone();
                self.save_journal(&job.id, &j)?;
                j
            }
            Some(_) => return Err("本地文件已修改，不能复用上传断点，请重新添加任务".into()),
            None => {
                let mut plan = client
                    .prepare_upload(&intent.target, &intent.encrypted_name, enc_size, &enc_hash)
                    .await?;
                first_part = plan.parts.first().cloned();
                plan.parts.clear();
                let j = UploadJournal {
                    plan,
                    finished_parts: 0,
                    source_size: enc_size,
                    source_modified: modified,
                };
                self.save_journal(&job.id, &j)?;
                j
            }
        };

        let map_enc_to_plain = |done_enc: u64| -> u64 {
            if enc_size == 0 || done_enc >= enc_size {
                plain_size
            } else {
                ((done_enc as u128 * plain_size as u128) / enc_size as u128) as u64
            }
        };

        if journal.plan.rapid || journal.plan.existing {
            control.check()?;
            self.checkpoint(&job.id, plain_size, plain_size)?;
            self.progress(job, plain_size, plain_size, 0.0, "completed");
            let _ = self
                .app
                .emit("crypt-directory-changed", &intent.root_folder_id);
            return Ok(());
        }

        if journal.plan.part_size == 0 {
            return Err("上传分片大小无效".into());
        }
        let part_count = enc_size.div_ceil(journal.plan.part_size).max(1);
        if journal.finished_parts > part_count {
            return Err("上传断点分片数无效".into());
        }
        let mut completed_enc = journal
            .finished_parts
            .saturating_mul(journal.plan.part_size)
            .min(enc_size);
        self.checkpoint(&job.id, map_enc_to_plain(completed_enc), plain_size)?;

        let mut renewed_part = false;
        'parts: while journal.finished_parts < part_count {
            let start_number = journal.finished_parts + 1;
            control.check()?;
            let parts = if start_number == 1 && first_part.is_some() {
                vec![first_part.take().expect("checked initial upload part")]
            } else {
                client
                    .refresh_upload_parts(&journal.plan, start_number, 1)
                    .await?
            };
            if parts.len() != 1 {
                return Err("云端未返回上传分片地址，请重试".into());
            }
            for part in parts {
                if part.part_number != journal.finished_parts + 1
                    || part.offset != completed_enc
                    || part.size != enc_size.saturating_sub(completed_enc).min(journal.plan.part_size)
                {
                    return Err("云端上传分片范围不匹配".into());
                }
                control.check()?;
                let part_source = self.open_source(&job.local_path).await?;
                let src_meta = part_source.metadata().await.map_err(|e| e.to_string())?;
                if src_meta.len() != plain_size || metadata_stamp(&src_meta) != modified {
                    return Err("上传期间本地文件发生变化，已暂停传输".into());
                }

                let cipher_part = cipher.clone();
                let part_end = part.offset + part.size;
                let enc_stream = futures_util::stream::try_unfold(
                    (part_source, part.offset),
                    move |(mut file, cur_enc)| {
                        let cipher = cipher_part.clone();
                        async move {
                            if cur_enc >= part_end {
                                return Ok::<_, std::io::Error>(None);
                            }
                            if cur_enc < FILE_HEADER_SIZE {
                                let end = part_end.min(FILE_HEADER_SIZE);
                                let slice = Bytes::copy_from_slice(
                                    &header[cur_enc as usize..end as usize],
                                );
                                return Ok(Some((slice, (file, end))));
                            }
                            let body_off = cur_enc - FILE_HEADER_SIZE;
                            let block_idx = body_off / BLOCK_SIZE;
                            let in_block = (body_off % BLOCK_SIZE) as usize;
                            let plain_off = block_idx * BLOCK_DATA_SIZE;
                            let plain_take =
                                (plain_size.saturating_sub(plain_off)).min(BLOCK_DATA_SIZE) as usize;
                            file.seek(std::io::SeekFrom::Start(plain_off)).await?;
                            let mut plain_buf = vec![0u8; plain_take];
                            file.read_exact(&mut plain_buf).await?;
                            let sealed = cipher
                                .seal_block(block_idx, &initial_nonce, &plain_buf)
                                .map_err(std::io::Error::other)?;
                            let avail = sealed.len() - in_block;
                            let want = ((part_end - cur_enc) as usize).min(avail);
                            let out = Bytes::copy_from_slice(&sealed[in_block..in_block + want]);
                            Ok(Some((out, (file, cur_enc + want as u64))))
                        }
                    },
                );

                let manager_app = self.app.clone();
                let transfer_activity = self.activity.clone();
                let id = job.id.clone();
                let stop = control.clone();
                let baseline_enc = completed_enc;
                let mut sent_enc = 0u64;
                let mut meter = Meter::new(0);
                let activity = Arc::new(AtomicU64::new(0));
                let activity_start = Instant::now();
                let stream_activity = activity.clone();

                let monitored = enc_stream.map(move |chunk| {
                    if stop.check().is_err() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "上传已暂停或取消",
                        ));
                    }
                    if let Ok(bytes) = &chunk {
                        stream_activity
                            .store(activity_start.elapsed().as_secs(), Ordering::Relaxed);
                        sent_enc += bytes.len() as u64;
                        if meter.ready() {
                            let done_plain = if enc_size == 0 {
                                0
                            } else {
                                (((baseline_enc + sent_enc) as u128 * plain_size as u128)
                                    / enc_size as u128) as u64
                            };
                            publish_transfer_progress(
                                &manager_app,
                                &transfer_activity,
                                TransferProgress {
                                    id: id.clone(),
                                    completed_bytes: done_plain.min(plain_size),
                                    total_bytes: plain_size,
                                    speed: meter.sample(sent_enc),
                                    phase: "uploading".into(),
                                },
                            );
                        }
                    }
                    chunk
                });

                let body = reqwest::Body::wrap_stream(monitored);
                let stalled = async {
                    loop {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        if activity_start
                            .elapsed()
                            .as_secs()
                            .saturating_sub(activity.load(Ordering::Relaxed))
                            >= 120
                        {
                            break;
                        }
                    }
                };
                let result = tokio::select! {
                    r = client.upload_part(&part, body) => r,
                    _ = stalled => return Err("上传超过 120 秒无进展，保留分片断点重试".into()),
                    _ = control.changed.notified() => { control.check()?; return Err("上传已停止".into()); }
                };
                if matches!(
                    &result,
                    Err(crate::cloud::UploadPartError::Rejected(
                        reqwest::StatusCode::FORBIDDEN
                    ))
                ) && !renewed_part
                {
                    renewed_part = true;
                    continue 'parts;
                }
                result?;
                renewed_part = false;
                control.check()?;
                journal.finished_parts = part.part_number;
                completed_enc += part.size;
                self.save_journal(&job.id, &journal)?;
                let done_plain = map_enc_to_plain(completed_enc);
                self.checkpoint(&job.id, done_plain, plain_size)?;
                self.progress(job, done_plain, plain_size, 0.0, "uploading");
            }
        }

        let final_src = self.open_source(&job.local_path).await?;
        let final_meta = final_src.metadata().await.map_err(|e| e.to_string())?;
        if final_meta.len() != plain_size || metadata_stamp(&final_meta) != modified {
            return Err("完成上传前本地文件发生变化，已停止提交".into());
        }
        control.check()?;
        self.progress(job, plain_size, plain_size, 0.0, "finalizing");
        client.complete_upload(&journal.plan).await?;
        self.checkpoint(&job.id, plain_size, plain_size)?;
        self.progress(job, plain_size, plain_size, 0.0, "completed");
        let _ = self
            .app
            .emit("crypt-directory-changed", &intent.root_folder_id);
        Ok(())
    }

    async fn run_crypt_download(
        &self,
        job: &TransferJob,
        client: &CloudClient,
        control: &Arc<Control>,
        intent: &CryptTransfer,
        cipher: Arc<OpenListCipher>,
    ) -> Result<(), String> {
        control.check()?;
        let expected_plain = cipher.decrypted_size(intent.enc_size)?;
        if expected_plain != intent.plain_size {
            return Err("加密文件长度与解密后目标大小不一致".into());
        }

        let destination = PathBuf::from(&job.local_path);
        let temporary = part_path(&job.local_path, &job.id);
        let previous: DownloadJournal = self
            .journal(&job.id)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        if destination.exists() {
            let durable = self.saved_completed(&job.id)?;
            let metadata = fs::metadata(&destination).await.map_err(|e| e.to_string())?;
            let published = same_file(&temporary, &destination);
            #[cfg(target_os = "android")]
            let published = published
                || (!temporary.exists()
                    && previous.publication.as_ref() == Some(&download_identity(&metadata)));
            if published && metadata.len() == durable && durable == expected_plain {
                self.progress(job, durable, durable, 0.0, "completed");
                return Ok(());
            }
            return Err("目标文件已存在，请更改保存位置".into());
        }

        let parent = destination.parent().ok_or("保存路径无效")?;
        fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&temporary)
            .await
            .map_err(|e| e.to_string())?;

        let partial_len = file.metadata().await.map_err(|e| e.to_string())?.len();
        let durable = self.saved_completed(&job.id)?;
        let trusted_plain = partial_len.min(durable);

        if expected_plain > 0 && trusted_plain == expected_plain {
            control.check()?;
            file.sync_all().await.map_err(|e| e.to_string())?;
            drop(file);
            self.publish_download(job, &temporary, &destination).await?;
            self.checkpoint(&job.id, expected_plain, expected_plain)?;
            self.progress(job, expected_plain, expected_plain, 0.0, "completed");
            return Ok(());
        }

        // Align resume point to 64 KiB authenticated block boundary.
        let mut completed_blocks = if expected_plain == 0 {
            0
        } else {
            (trusted_plain.min(expected_plain)) / BLOCK_DATA_SIZE
        };
        let mut plain_offset = completed_blocks * BLOCK_DATA_SIZE;
        if partial_len != plain_offset {
            file.set_len(plain_offset)
                .await
                .map_err(|e| e.to_string())?;
        }
        ensure_download_space(parent, expected_plain.saturating_sub(plain_offset))?;

        let source_client;
        let download_client = if let Some(sid) = &intent.target.source_id {
            let owner = client.account_config().await.id;
            source_client = crate::sessions::Sessions::load(&self.store)?
                .source(&owner, sid, true)?
                .client(client)
                .await?;
            &source_client
        } else {
            client
        };

        let mut header_nonce: Option<[u8; FILE_NONCE_SIZE]> = None;
        if completed_blocks > 0 {
            let link = download_client.link(&intent.target).await?;
            let hdr_resp = self
                .http
                .get(&link)
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .header(reqwest::header::RANGE, "bytes=0-31")
                .send()
                .await
                .map_err(|e| e.without_url().to_string())?;
            if hdr_resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                let hdr_bytes = hdr_resp
                    .bytes()
                    .await
                    .map_err(|e| e.without_url().to_string())?;
                if hdr_bytes.len() == 32 {
                    header_nonce = Some(OpenListCipher::parse_header(&hdr_bytes)?);
                }
            }
            if header_nonce.is_none() {
                completed_blocks = 0;
                plain_offset = 0;
                file.set_len(0).await.map_err(|e| e.to_string())?;
            }
        }

        let request_enc_offset = if completed_blocks > 0 {
            FILE_HEADER_SIZE + completed_blocks * BLOCK_SIZE
        } else {
            0
        };

        let (response, actual_req_offset) = download_with_fresh_link(
            &self.http,
            || download_client.link(&intent.target),
            request_enc_offset,
            previous.validator.as_deref(),
            control,
        )
        .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("下载加密文件返回 HTTP {}", status.as_u16()));
        }
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok());
        let disposition = resume_disposition(
            status.as_u16(),
            actual_req_offset,
            content_range,
            intent.enc_size,
        )?;
        let validator = response
            .headers()
            .get(reqwest::header::ETAG)
            .filter(|v| !v.as_bytes().starts_with(b"W/"))
            .or_else(|| response.headers().get(reqwest::header::LAST_MODIFIED))
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        if disposition == ResumeDisposition::Restart || actual_req_offset == 0 {
            completed_blocks = 0;
            plain_offset = 0;
            header_nonce = None;
            file.set_len(0).await.map_err(|e| e.to_string())?;
        } else if let (Some(old), Some(new)) = (&previous.validator, &validator) {
            if old != new {
                return Err("云端加密文件版本已变化，请重新添加下载任务".into());
            }
        }

        self.save_journal(
            &job.id,
            &DownloadJournal {
                validator,
                publication: None,
            },
        )?;
        file.seek(std::io::SeekFrom::Start(plain_offset))
            .await
            .map_err(|e| e.to_string())?;
        self.checkpoint(&job.id, plain_offset, expected_plain)?;

        let mut stream = response.bytes_stream();
        let mut header_buf = Vec::with_capacity(32);
        let mut block_buf = Vec::with_capacity(BLOCK_SIZE as usize);
        let mut cur_block_idx = completed_blocks;
        let mut cur_plain = plain_offset;
        let mut enc_received = if header_nonce.is_some() {
            FILE_HEADER_SIZE + completed_blocks * BLOCK_SIZE
        } else {
            0
        };
        let mut meter = Meter::new(cur_plain);

        loop {
            control.check()?;
            let next = tokio::select! {
                n = stream.next() => n,
                _ = control.changed.notified() => {
                    control.check()?;
                    continue;
                }
            };
            let Some(next) = next else { break };
            let chunk = next.map_err(|e| e.without_url().to_string())?;
            if enc_received.saturating_add(chunk.len() as u64) > intent.enc_size {
                return Err("云端返回的加密数据超出预期大小".into());
            }
            enc_received += chunk.len() as u64;
            let mut slice: &[u8] = &chunk;

            if header_nonce.is_none() {
                let need = 32 - header_buf.len();
                let take = slice.len().min(need);
                header_buf.extend_from_slice(&slice[..take]);
                slice = &slice[take..];
                if header_buf.len() == 32 {
                    header_nonce = Some(OpenListCipher::parse_header(&header_buf)?);
                } else {
                    continue;
                }
            }

            let nonce = header_nonce.as_ref().unwrap();
            while !slice.is_empty() {
                let need = (BLOCK_SIZE as usize) - block_buf.len();
                let take = slice.len().min(need);
                block_buf.extend_from_slice(&slice[..take]);
                slice = &slice[take..];

                if block_buf.len() == BLOCK_SIZE as usize {
                    let plain_block = cipher.open_block(cur_block_idx, nonce, &block_buf)?;
                    file.write_all(&plain_block)
                        .await
                        .map_err(|e| e.to_string())?;
                    cur_plain += plain_block.len() as u64;
                    cur_block_idx += 1;
                    block_buf.clear();
                }
            }

            if meter.ready() {
                self.progress(
                    job,
                    cur_plain,
                    expected_plain,
                    meter.sample(cur_plain),
                    "decrypting",
                );
                if meter.checkpoint_ready() {
                    file.sync_data().await.map_err(|e| e.to_string())?;
                    self.checkpoint(&job.id, cur_plain, expected_plain)?;
                }
            }
        }

        if enc_received != intent.enc_size {
            file.sync_data().await.map_err(|e| e.to_string())?;
            self.checkpoint(&job.id, cur_plain, expected_plain)?;
            return Err(format!(
                "加密下载流中断，已保存分块断点（{cur_plain}/{expected_plain} 字节）"
            ));
        }

        if header_nonce.is_none() {
            return Err("加密文件缺少 32 字节头部".into());
        }
        if !block_buf.is_empty() {
            if block_buf.len() as u64 <= BLOCK_HEADER_SIZE {
                return Err("加密文件尾块不完整".into());
            }
            let nonce = header_nonce.as_ref().unwrap();
            let plain_block = cipher.open_block(cur_block_idx, nonce, &block_buf)?;
            file.write_all(&plain_block)
                .await
                .map_err(|e| e.to_string())?;
            cur_plain += plain_block.len() as u64;
            block_buf.clear();
        }

        if cur_plain != expected_plain {
            return Err("解密后文件大小与预期不符".into());
        }

        file.flush().await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        control.check()?;
        self.checkpoint(&job.id, cur_plain, cur_plain)?;
        drop(file);
        self.publish_download(job, &temporary, &destination).await?;
        self.checkpoint(&job.id, cur_plain, cur_plain)?;
        self.progress(job, cur_plain, cur_plain, 0.0, "completed");
        Ok(())
    }
}
