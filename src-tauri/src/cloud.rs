//! Native, portable 139 Cloud protocol adapter. Credentials never enter URLs or logs.
use crate::auth;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{FixedOffset, Utc};
use md5::{Digest, Md5};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountConfig {
    pub id: String,
    pub name: String,
    pub authorization: String,
    pub username: String,
    pub password: String,
    pub sms_code: String,
    pub mail_cookies: String,
    pub space: String,
    pub root_id: String,
    pub cloud_id: String,
    pub user_domain_id: String,
    pub link_id: String,
    pub custom_upload_part_size: u64,
    pub use_large_thumbnail: bool,
}
impl Default for AccountConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "我的移动云盘".into(),
            authorization: String::new(),
            username: String::new(),
            password: String::new(),
            sms_code: String::new(),
            mail_cookies: String::new(),
            space: "personal_new".into(),
            root_id: String::new(),
            cloud_id: String::new(),
            user_domain_id: String::new(),
            link_id: String::new(),
            custom_upload_part_size: 0,
            use_large_thumbnail: false,
        }
    }
}
impl AccountConfig {
    pub fn keep_personal_space(&mut self) {
        if self.space != "personal_new" {
            self.root_id.clear();
        }
        self.space = "personal_new".into();
        self.cloud_id.clear();
        self.link_id.clear();
    }
    /// Keep saved credentials usable after a provider space is retired. Directory
    /// identifiers must not cross spaces; existing transfers keep their scope.
    pub fn migrate_saved_space(&mut self) {
        if matches!(self.space.as_str(), "personal" | "family") {
            self.space = "personal_new".into();
            self.root_id.clear();
            self.cloud_id.clear();
            self.link_id.clear();
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_at: String,
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePage {
    pub files: Vec<CloudFile>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadPart {
    pub part_number: u64,
    pub offset: u64,
    pub size: u64,
    pub url: String,
}

#[derive(Debug)]
pub enum UploadPartError {
    Rejected(reqwest::StatusCode),
    Transport(String),
}

impl From<UploadPartError> for String {
    fn from(error: UploadPartError) -> Self {
        match error {
            UploadPartError::Rejected(status) => format!("分片上传返回 HTTP {}", status.as_u16()),
            UploadPartError::Transport(message) => message,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadPlan {
    pub file_id: String,
    pub upload_id: String,
    pub name: String,
    pub size: u64,
    pub part_size: u64,
    pub parts: Vec<UploadPart>,
    pub rapid: bool,
    pub existing: bool,
    pub content_hash: String,
    pub group_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}
#[derive(Clone, Default)]
struct Routes {
    personal: String,
    group: String,
    group_album: String,
}
#[derive(Clone)]
pub struct CloudClient {
    pub http: Client,
    pub transfer_http: Client,
    config: Arc<RwLock<AccountConfig>>,
    routes: Arc<RwLock<Routes>>,
    auth_lock: Arc<Mutex<()>>,
}

fn nonce(length: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}
pub fn encode_uri_component(text: &str) -> String {
    let mut output = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(byte) {
            output.push(*byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}
pub fn sign(body: &str, timestamp: &str, random: &str) -> String {
    let mut encoded = encode_uri_component(body).into_bytes();
    encoded.sort_unstable();
    let sorted64 = STANDARD.encode(encoded);
    let first = format!("{:x}", Md5::digest(sorted64.as_bytes()));
    let second = format!(
        "{:x}",
        Md5::digest(format!("{timestamp}:{random}").as_bytes())
    );
    format!("{:X}", Md5::digest(format!("{first}{second}").as_bytes()))
}
fn string(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v[key].as_u64().map(|n| n.to_string()).unwrap_or_default())
}
fn number(v: &Value, key: &str) -> u64 {
    v[key]
        .as_u64()
        .or_else(|| v[key].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or_default()
}
fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn optional(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}
fn join_provider(parent: &str, id: &str) -> String {
    if parent.ends_with(&format!("/{id}")) || parent == id {
        parent.to_owned()
    } else {
        format!("{}/{id}", parent.trim_end_matches('/'))
    }
}
fn headers(values: &[(&str, &str)]) -> Result<HeaderMap, String> {
    let mut out = HeaderMap::new();
    for (key, value) in values {
        out.insert(
            HeaderName::from_bytes(key.as_bytes()).map_err(|e| e.to_string())?,
            HeaderValue::from_str(value).map_err(|_| format!("请求头 {key} 无效"))?,
        );
    }
    Ok(out)
}
fn validate_response(value: &Value) -> Result<(), String> {
    if !value.is_object() {
        return Err("云盘业务响应格式无效".into());
    }
    // The batch adapter reports a root result object instead of the
    // normal success envelope, including on some explicit success=false replies.
    if value["result"]["resultCode"] == "0" || value["result"]["resultCode"] == 0 {
        return Ok(());
    }
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(format!(
            "{}{}",
            value["message"].as_str().unwrap_or("云盘请求失败"),
            value["code"]
                .as_str()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default()
        ));
    }
    let mut successful = value["success"] == true;
    for result in [&value["result"], &value["data"]["result"]] {
        if let Some(code) = result.get("resultCode") {
            let valid = code == "0" || code == 0;
            if !valid {
                return Err(format!(
                    "云盘操作失败：{} ({code})",
                    result["resultDesc"].as_str().unwrap_or_default()
                ));
            }
            successful = true;
        }
    }
    if successful {
        Ok(())
    } else {
        Err("云盘业务响应缺少明确的成功结果".into())
    }
}
fn validate_host(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value).map_err(|_| "云盘路由地址无效")?;
    let host = url.host_str().ok_or("云盘路由缺少主机")?;
    if url.scheme() != "https" || !(host == "yun.139.com" || host.ends_with(".yun.139.com")) {
        return Err("云盘返回了不可信的路由地址".into());
    }
    Ok(value.trim_end_matches('/').to_owned())
}

impl CloudClient {
    pub async fn for_browse_source(&self, kind: &str, reference: &str) -> Result<Self, String> {
        let mut config = self.account_config().await;
        config.space = kind.into();
        let root = if kind == "group" {
            let routes = self.routes.read().await;
            if routes.group.is_empty() || routes.group_album.is_empty() {
                return Err("云盘未返回群组路由".into());
            }
            config.cloud_id = reference.into();
            reference
        } else {
            config.link_id = reference.into();
            "root"
        };
        config.root_id = root.into();
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            ..self.clone()
        })
    }

    pub fn new(config: AccountConfig) -> Result<Self, String> {
        if !["personal_new", "group", "share"].contains(&config.space.as_str()) {
            return Err("未知的云盘空间类型".into());
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(90))
            .user_agent("CMCloudPlusPlus/0.1")
            .build()
            .map_err(|e| e.to_string())?;
        let transfer_http = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            // Reqwest starts its read timer before a streaming request finishes.
            // Uploads use the transfer worker's progress-based stall watchdog.
            .pool_idle_timeout(Duration::from_secs(90))
            .user_agent("CMCloudPlusPlus/0.1")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            transfer_http,
            config: Arc::new(RwLock::new(config)),
            routes: Arc::new(RwLock::new(Routes::default())),
            auth_lock: Arc::new(Mutex::new(())),
        })
    }
    pub async fn account_config(&self) -> AccountConfig {
        self.config.read().await.clone()
    }
    pub async fn set_account_id(&self, id: String) {
        self.config.write().await.id = id;
    }
    pub async fn initialize(&self) -> Result<AccountConfig, String> {
        let _guard = self.auth_lock.lock().await;
        let mut cfg = self.account_config().await;
        let result = auth::authenticate(&self.http, &mut cfg).await;
        *self.config.write().await = cfg.clone();
        result?;
        if !cfg.authorization.is_empty() {
            let account = auth::account_from_authorization(&cfg.authorization)?;
            let value=self.signed_post("https://user-njs.yun.139.com/user/route/qryRoutePolicy",&json!({"userInfo":{"userType":1,"accountType":1,"accountName":account},"modAddrType":1}),false).await?;
            let mut routes = Routes::default();
            for item in array(&value["data"], "routePolicyList") {
                let module = string(item, "modName");
                if !["personal", "group", "family"].contains(&module.as_str()) {
                    continue;
                }
                let address = string(item, "httpsUrl");
                if address.is_empty() {
                    continue;
                }
                let address = validate_host(&address)?;
                match module.as_str() {
                    "personal" => routes.personal = address,
                    "group" => routes.group = address,
                    "family" => routes.group_album = address,
                    _ => {}
                }
            }
            if routes.personal.is_empty() {
                return Err("云盘未返回个人空间路由".into());
            }
            if cfg.space == "group" && (routes.group.is_empty() || routes.group_album.is_empty()) {
                return Err("云盘未返回群组路由".into());
            }
            *self.routes.write().await = routes;
        }
        match cfg.space.as_str() {
            "personal_new" => {
                if cfg.root_id.is_empty() {
                    cfg.root_id = "/".into();
                }
            }
            "group" => {
                if cfg.cloud_id.is_empty() {
                    return Err("共享群需要填写 cloudId".into());
                }
                if cfg.root_id.is_empty() {
                    cfg.root_id = cfg.cloud_id.clone();
                }
            }
            "share" => {
                if parse_share_entries(&cfg.link_id).is_empty() {
                    return Err("请添加有效的分享链接或 linkId".into());
                }
                if cfg.root_id.is_empty() {
                    cfg.root_id = "root".into();
                }
            }
            _ => {}
        }
        *self.config.write().await = cfg.clone();
        Ok(cfg)
    }
    pub async fn refresh_credentials(&self) -> Result<AccountConfig, String> {
        let _guard = self.auth_lock.lock().await;
        let mut cfg = self.account_config().await;
        let result = auth::authenticate(&self.http, &mut cfg).await;
        *self.config.write().await = cfg.clone();
        result?;
        Ok(cfg)
    }
    pub async fn root(&self) -> CloudFile {
        let cfg = self.account_config().await;
        CloudFile {
            id: cfg.root_id,
            name: cfg.name,
            is_dir: true,
            ..Default::default()
        }
    }
    async fn account(&self) -> Result<String, String> {
        let cfg = self.config.read().await;
        if cfg.authorization.is_empty() && cfg.space == "share" {
            return Ok(String::new());
        }
        auth::account_from_authorization(&cfg.authorization)
    }
    async fn signed_post(&self, url: &str, body: &Value, modern: bool) -> Result<Value, String> {
        let cfg = self.account_config().await;
        let timestamp = Utc::now()
            .with_timezone(&FixedOffset::east_opt(8 * 3600).expect("valid timezone"))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let random = nonce(16);
        let text = serde_json::to_string(body).map_err(|e| e.to_string())?;
        let mut h = headers(&[
            ("accept", "application/json, text/plain, */*"),
            ("cms-device", "default"),
            ("mcloud-channel", "1000101"),
            ("mcloud-client", "10701"),
            ("mcloud-version", "7.14.0"),
            ("origin", "https://yun.139.com"),
            ("referer", "https://yun.139.com/w/"),
            (
                "x-deviceinfo",
                "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||",
            ),
            ("x-huawei-channelsrc", "10000034"),
            ("x-inner-ntwk", "2"),
            ("x-m4c-caller", "PC"),
            ("x-m4c-src", "10002"),
            ("inner-hcy-router-https", "1"),
        ])?;
        h.insert(
            "authorization",
            format!("Basic {}", cfg.authorization)
                .parse()
                .map_err(|_| "凭据格式无效")?,
        );
        h.insert(
            "mcloud-sign",
            format!("{timestamp},{random},{}", sign(&text, &timestamp, &random))
                .parse()
                .map_err(|_| "签名格式无效")?,
        );
        h.insert("x-svctype", "1".parse().unwrap());
        if modern {
            for (key, value) in [
                ("caller", "web"),
                ("mcloud-route", "001"),
                ("x-yun-api-version", "v1"),
                ("x-yun-app-channel", "10000034"),
                ("x-yun-channel-source", "10000034"),
                (
                    "x-yun-client-info",
                    "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||dW5kZWZpbmVk||",
                ),
                ("x-yun-module-type", "100"),
                ("x-yun-svc-type", "1"),
            ] {
                h.insert(
                    HeaderName::from_static(key),
                    HeaderValue::from_static(value),
                );
            }
        }
        let response = self
            .http
            .post(url)
            .headers(h)
            .header("Content-Type", "application/json")
            .body(text)
            .send()
            .await
            .map_err(|e| format!("云盘连接失败：{}", e.without_url()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("云盘服务返回 HTTP {}", status.as_u16()));
        }
        let bytes = auth::read_body_limited(response, 8 * 1024 * 1024).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "云盘响应格式无效")?;
        validate_response(&value)?;
        Ok(value)
    }
    async fn legacy_post(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.signed_post(&format!("https://yun.139.com{path}"), body, false)
            .await
    }
    async fn modern_post(&self, path: &str, body: &Value, dynamic: bool) -> Result<Value, String> {
        let routes = self.routes.read().await;
        let host = if dynamic {
            &routes.group
        } else {
            &routes.personal
        };
        if host.is_empty() {
            return Err("请先连接云盘账号".into());
        }
        let url = format!("{host}{path}");
        drop(routes);
        self.signed_post(&url, body, true).await
    }
    async fn album_post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let cfg = self.account_config().await;
        let host = self.routes.read().await.group_album.clone();
        if host.is_empty() {
            return Err("群组复制路由不可用".into());
        }
        let mut h=headers(&[("x-svctype","2"),("hcy-cool-flag","1"),("api-version","v2"),("x-huawei-channelsrc","10246600"),("x-sdk-channelsrc",""),("x-mm-source","0"),("x-deviceinfo","1|127.0.0.1|1|12.3.2|Xiaomi|23116PN5BC||02-00-00-00-00-00|android 15|1440x3200|android|zh||||032|0|"),("content-type","application/json; charset=utf-8"),("user-agent","okhttp/4.11.0")])?;
        h.insert(
            "authorization",
            format!("Basic {}", cfg.authorization)
                .parse()
                .map_err(|_| "凭据格式无效")?,
        );
        let result = auth::encrypted_post(
            &self.http,
            &format!("{host}/andAlbum/openApi{path}"),
            body,
            h,
            auth::KEY_ONE,
        )
        .await?;
        validate_response(&result)?;
        Ok(result)
    }
    async fn share_post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let cfg = self.account_config().await;
        let mut h = headers(&[
            ("accept", "application/json, text/plain, */*"),
            ("content-type", "application/json;charset=UTF-8"),
            (
                "x-deviceinfo",
                "||9|12.27.0|firefox|140.0|||linux unknown|1920X526|zh-CN|||",
            ),
            ("hcy-cool-flag", "1"),
            ("cms-device", "default"),
            ("x-m4c-caller", "PC"),
            ("x-yun-api-version", "v1"),
            ("origin", "https://yun.139.com"),
            ("referer", "https://yun.139.com/"),
        ])?;
        if !cfg.authorization.is_empty() {
            h.insert(
                "authorization",
                format!("Basic {}", cfg.authorization)
                    .parse()
                    .map_err(|_| "凭据格式无效")?,
            );
        }
        let value = auth::encrypted_post(
            &self.http,
            &format!("https://share-kd-njs.yun.139.com/yun-share{path}"),
            body,
            h,
            &hex::encode("PVGDwmcvfs1uV3d1"),
        )
        .await?;
        validate_response(&value)?;
        Ok(value)
    }
    async fn common(&self) -> Result<Value, String> {
        Ok(json!({"account":self.account().await?,"accountType":1}))
    }
    async fn group_common(&self, mut data: Value) -> Result<Value, String> {
        let cfg = self.account_config().await;
        data["catalogType"] = json!(3);
        data["cloudID"] = json!(cfg.cloud_id);
        data["cloudType"] = json!(1);
        data["commonAccountInfo"] = self.common().await?;
        Ok(data)
    }
    async fn dir_path(&self, file: &CloudFile) -> String {
        if file.path.is_empty() {
            file.id.clone()
        } else {
            join_provider(&file.path, &file.id)
        }
    }

    pub async fn list_page(
        &self,
        parent: &CloudFile,
        cursor: Option<String>,
    ) -> Result<FilePage, String> {
        if !parent.is_dir {
            return Err("只能浏览文件夹".into());
        }
        let cfg = self.account_config().await;
        if cfg.space == "share" {
            return self.share_list(parent).await;
        }
        match cfg.space.as_str() {
            "personal_new" => {
                let value=self.modern_post("/file/list",&json!({"imageThumbnailStyleList":["Small","Large"],"orderBy":"updated_at","orderDirection":"DESC","pageInfo":{"pageCursor":cursor.unwrap_or_default(),"pageSize":100},"parentFileId":parent.id}),false).await?;
                let data = &value["data"];
                let files = array(data, "items")
                    .iter()
                    .map(|item| {
                        let thumbs = array(item, "thumbnailUrls");
                        let thumb = if cfg.use_large_thumbnail {
                            thumbs
                                .iter()
                                .find(|t| string(t, "style").contains("Large"))
                                .or(thumbs.last())
                        } else {
                            thumbs.last()
                        };
                        CloudFile {
                            id: string(item, "fileId"),
                            name: string(item, "name"),
                            path: parent.id.clone(),
                            is_dir: string(item, "type") == "folder",
                            size: number(item, "size"),
                            modified_at: string(item, "updatedAt"),
                            thumbnail: thumb.and_then(|v| optional(string(v, "url"))),
                            source_id: None,
                        }
                    })
                    .collect();
                Ok(FilePage {
                    files,
                    next_cursor: optional(string(data, "nextPageCursor")),
                })
            }
            "group" => {
                let start = cursor
                    .as_deref()
                    .unwrap_or("1")
                    .parse::<u64>()
                    .map_err(|_| "分页参数无效")?;
                let catalog = parent.id.rsplit('/').next().unwrap_or(&parent.id);
                let body=self.group_common(json!({"groupID":cfg.cloud_id,"catalogID":catalog,"contentSortType":0,"sortDirection":1,"startNumber":start,"endNumber":start+99,"path":self.dir_path(parent).await})).await?;
                let value = self
                    .legacy_post(
                        "/orchestration/group-rebuild/content/v1.0/queryGroupContentList",
                        &body,
                    )
                    .await?;
                let data = &value["data"]["getGroupContentResult"];
                let files = legacy_files(
                    data,
                    "catalogList",
                    "contentList",
                    &string(data, "parentCatalogID"),
                );
                let next = (start + 99 < number(data, "nodeCount") && !files.is_empty())
                    .then(|| (start + 100).to_string());
                Ok(FilePage {
                    files,
                    next_cursor: next,
                })
            }
            _ => Err("未知空间类型".into()),
        }
    }
    pub async fn list_all(&self, parent: &CloudFile) -> Result<Vec<CloudFile>, String> {
        let mut files = Vec::new();
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut ids = HashSet::new();
        loop {
            let page = self.list_page(parent, cursor.clone()).await?;
            let mut new_count = 0;
            for file in page.files {
                if ids.insert(file.id.clone()) {
                    files.push(file);
                    new_count += 1;
                }
            }
            match page.next_cursor {
                Some(next) => {
                    if new_count == 0 || !cursors.insert(next.clone()) {
                        return Err("云盘分页重复，请刷新后重试".into());
                    }
                    cursor = Some(next);
                }
                None => break,
            }
        }
        Ok(files)
    }
    pub async fn link(&self, file: &CloudFile) -> Result<String, String> {
        if file.is_dir {
            return Err("文件夹不能直接下载".into());
        }
        let cfg = self.account_config().await;
        let value = match cfg.space.as_str() {
            "personal_new" => {
                let value = self
                    .modern_post("/file/getDownloadUrl", &json!({"fileId":file.id}), false)
                    .await?;
                let data = &value["data"];
                let result = if data["cdnSwitch"] == true && !string(data, "cdnUrl").is_empty() {
                    string(data, "cdnUrl")
                } else {
                    string(data, "url")
                };
                return valid_download_url(result);
            }
            "group" => {
                self.legacy_post(
                    "/orchestration/group-rebuild/groupManage/v1.0/getGroupFileDownLoadURL",
                    &self
                        .group_common(
                            json!({"contentID":file.id,"groupID":cfg.cloud_id,"path":file.path}),
                        )
                        .await?,
                )
                .await?
            }
            "share" => return self.share_link(file, false).await,
            _ => return Err("未知空间类型".into()),
        };
        valid_download_url(string(&value["data"], "downloadURL"))
    }
    pub async fn preview(&self, file: &CloudFile) -> Result<Value, String> {
        let cfg = self.account_config().await;
        if cfg.space == "share" {
            return Ok(json!({"url":self.share_link(file,true).await?}));
        }
        if cfg.space == "personal_new" {
            let value = self
                .modern_post(
                    "/videoPreview/getPreviewInfo",
                    &json!({"category":"video","fileId":file.id}),
                    false,
                )
                .await?;
            return Ok(value["data"].clone());
        }
        Ok(json!({"url":self.link(file).await?}))
    }
    pub async fn mkdir(&self, parent: &CloudFile, name: &str) -> Result<(), String> {
        self.make_directory_response(parent, name).await?;
        Ok(())
    }
    async fn make_directory_response(
        &self,
        parent: &CloudFile,
        name: &str,
    ) -> Result<Value, String> {
        validate_name(name)?;
        if !parent.is_dir {
            return Err("新建目录的目标需要为文件夹".into());
        }
        let cfg = self.account_config().await;
        let common = self.common().await?;
        match cfg.space.as_str() {
            "personal_new" => {
                self.modern_post("/file/create",&json!({"parentFileId":parent.id,"name":name,"description":"","type":"folder","fileRenameMode":"force_rename"}),false).await
            }
            "group" => {
                self.legacy_post("/orchestration/group-rebuild/catalog/v1.0/createGroupCatalog",&json!({"catalogName":name,"commonAccountInfo":common,"groupID":cfg.cloud_id,"parentFileId":parent.id,"path":self.dir_path(parent).await})).await
            }
            _ => Err("分享空间为只读".into()),
        }
    }
    /// Call after a cached parent listing established the name is absent.
    /// Uses the returned identifier when available, and bounded refreshes for
    /// older APIs that return only an operation result.
    pub async fn create_directory(
        &self,
        parent: &CloudFile,
        name: &str,
    ) -> Result<CloudFile, String> {
        let value = self.make_directory_response(parent, name).await?;
        let data = &value["data"];
        let id = [
            string(data, "fileId"),
            string(data, "catalogID"),
            string(&data["createCatalogResult"], "catalogID"),
        ]
        .into_iter()
        .find(|id| !id.is_empty());
        if let Some(id) = id {
            let returned_name = [
                string(data, "name"),
                string(data, "fileName"),
                string(data, "catalogName"),
            ]
            .into_iter()
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| name.into());
            if returned_name != name {
                return Err(format!(
                    "服务端将目录改名为 {returned_name}，请刷新后选择该目录"
                ));
            }
            let cfg = self.account_config().await;
            let parent_path = self.dir_path(parent).await;
            let path = if cfg.space == "group" {
                join_provider(&parent_path, &id)
            } else {
                parent.id.clone()
            };
            return Ok(CloudFile {
                id,
                name: name.into(),
                path,
                is_dir: true,
                ..Default::default()
            });
        }
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
            }
            let mut cursor = None;
            let mut seen = HashSet::new();
            loop {
                let page = self.list_page(parent, cursor).await?;
                if let Some(file) = page
                    .files
                    .into_iter()
                    .find(|file| file.is_dir && file.name == name)
                {
                    return Ok(file);
                }
                match page.next_cursor {
                    Some(next) if seen.insert(next.clone()) => cursor = Some(next),
                    Some(_) => return Err("新建目录后分页没有前进".into()),
                    None => break,
                }
            }
        }
        Err("目录创建已提交，但暂时未能读取目录标识，请刷新后重试".into())
    }
    pub async fn rename(&self, file: &CloudFile, name: &str) -> Result<(), String> {
        validate_name(name)?;
        let cfg = self.account_config().await;
        let common = self.common().await?;
        match cfg.space.as_str() {
            "personal_new" => {
                self.modern_post(
                    "/file/update",
                    &json!({"fileId":file.id,"name":name,"description":""}),
                    false,
                )
                .await?;
            }
            "group" => {
                let (path, body) = if file.is_dir {
                    (
                        "/orchestration/group-rebuild/catalog/v1.0/modifyGroupCatalog",
                        json!({"groupID":cfg.cloud_id,"modifyCatalogID":file.id,"modifyCatalogName":name,"path":file.path,"commonAccountInfo":common}),
                    )
                } else {
                    (
                        "/orchestration/group-rebuild/content/v1.0/modifyGroupContent",
                        json!({"groupID":cfg.cloud_id,"contentID":file.id,"contentName":name,"path":file.path,"commonAccountInfo":common}),
                    )
                };
                self.legacy_post(path, &body).await?;
            }
            _ => return Err("分享空间为只读".into()),
        }
        Ok(())
    }
    pub async fn move_file(&self, file: &CloudFile, destination: &CloudFile) -> Result<(), String> {
        self.move_or_copy(file, destination, false).await
    }
    pub async fn copy(&self, file: &CloudFile, destination: &CloudFile) -> Result<(), String> {
        self.move_or_copy(file, destination, true).await
    }
    async fn move_or_copy(
        &self,
        file: &CloudFile,
        dst: &CloudFile,
        copy: bool,
    ) -> Result<(), String> {
        if !dst.is_dir {
            return Err("目标需要为文件夹".into());
        }
        if file.id == dst.id {
            return Err("不能移动或复制到自身".into());
        }
        let cfg = self.account_config().await;
        let (contents, catalogs) = if file.is_dir {
            (Vec::<String>::new(), vec![file.id.clone()])
        } else {
            (vec![file.id.clone()], Vec::<String>::new())
        };
        let common = self.common().await?;
        match cfg.space.as_str() {
            "personal_new" => {
                self.modern_post(
                    if copy {
                        "/file/batchCopy"
                    } else {
                        "/file/batchMove"
                    },
                    &json!({"fileIds":[file.id],"toParentFileId":dst.id}),
                    false,
                )
                .await?;
            }
            "group" if copy => {
                let full = self.dir_path(file).await;
                let full = if full.starts_with("root:") {
                    full
                } else {
                    format!("root:/{full}")
                };
                let dest = self.dir_path(dst).await;
                let dest = if dest.starts_with("root:") {
                    dest
                } else {
                    format!("root:/{dest}")
                };
                let (contents, catalogs) = if file.is_dir {
                    (vec![], vec![full])
                } else {
                    (vec![full], vec![])
                };
                self.album_post("/copyContentCatalog",&json!({"commonAccountInfo":{"accountType":"1","accountUserId":cfg.user_domain_id},"destCatalogID":dest,"destCloudID":cfg.cloud_id,"sourceCatalogIDs":catalogs,"sourceCloudID":cfg.cloud_id,"sourceContentIDs":contents})).await?;
            }
            "group" => {
                self.legacy_post("/orchestration/group-rebuild/task/v1.0/createBatchOprTask",&json!({"taskType":3,"srcType":2,"srcGroupID":cfg.cloud_id,"destType":2,"destGroupID":cfg.cloud_id,"destPath":self.dir_path(dst).await,"contentList":contents,"catalogList":catalogs,"commonAccountInfo":common})).await?;
            }
            _ => return Err("分享空间为只读".into()),
        }
        Ok(())
    }
    pub async fn remove(&self, file: &CloudFile) -> Result<(), String> {
        let cfg = self.account_config().await;
        let common = self.common().await?;
        match cfg.space.as_str() {
            "personal_new" => {
                self.modern_post(
                    "/recyclebin/batchTrash",
                    &json!({"fileIds":[file.id]}),
                    false,
                )
                .await?;
            }
            "group" => {
                let full = if file.is_dir && !file.path.is_empty() {
                    file.path.clone()
                } else {
                    self.dir_path(file).await
                };
                let (contents, catalogs) = if file.is_dir {
                    (vec![], vec![full])
                } else {
                    (vec![full], vec![])
                };
                self.legacy_post("/orchestration/group-rebuild/task/v1.0/createBatchOprTask",&json!({"taskType":2,"srcGroupID":cfg.cloud_id,"contentList":contents,"catalogList":catalogs,"commonAccountInfo":common})).await?;
            }
            _ => return Err("分享空间为只读".into()),
        }
        Ok(())
    }

    pub async fn remove_personal_ids(&self, ids: &[String]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let cfg = self.account_config().await;
        if cfg.space != "personal_new" {
            return Err("仅支持个人云批量移入回收站".into());
        }
        for chunk in ids.chunks(50) {
            self.modern_post("/recyclebin/batchTrash", &json!({"fileIds": chunk}), false)
                .await?;
        }
        Ok(())
    }

    pub async fn file_digest(&self, file_id: &str) -> Result<(u64, String, Option<i64>), String> {
        let cfg = self.account_config().await;
        if cfg.space != "personal_new" {
            return Err("仅支持个人云文件校验信息读取".into());
        }
        let value = self
            .modern_post("/file/get", &json!({"fileId": file_id}), false)
            .await?;
        let data = &value["data"];
        if string(data, "fileId") != file_id || string(data, "type") != "file" {
            return Err("所选云盘文件不存在或类型已变化".into());
        }
        let size = number(data, "size");
        let algo = string(data, "contentHashAlgorithm");
        let hash = string(data, "contentHash").to_ascii_lowercase();
        if !algo.eq_ignore_ascii_case("SHA256")
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("云盘未提供完整的 SHA256 摘要".into());
        }
        let modified = parse_cloud_millis(&string(data, "updatedAt"));
        Ok((size, hash, modified))
    }

    pub async fn list_personal_entries(
        &self,
        parent_id: &str,
    ) -> Result<Vec<(CloudFile, String, Option<i64>)>, String> {
        let cfg = self.account_config().await;
        if cfg.space != "personal_new" {
            return Err("仅支持个人云目录枚举".into());
        }
        let mut out = Vec::new();
        let mut cursor = String::new();
        let mut seen_cursors = HashSet::new();
        let mut seen_ids = HashSet::new();
        loop {
            let value = self
                .modern_post(
                    "/file/list",
                    &json!({
                        "orderBy": "name",
                        "orderDirection": "ASC",
                        "pageInfo": {"pageCursor": cursor, "pageSize": 200},
                        "parentFileId": parent_id
                    }),
                    false,
                )
                .await?;
            let data = &value["data"];
            for item in array(data, "items") {
                let id = string(item, "fileId");
                if id.is_empty() || !seen_ids.insert(id.clone()) {
                    continue;
                }
                let is_dir = string(item, "type") == "folder";
                let updated = string(item, "updatedAt");
                let modified = parse_cloud_millis(&updated);
                let hash = if !is_dir
                    && string(item, "contentHashAlgorithm").eq_ignore_ascii_case("SHA256")
                {
                    let h = string(item, "contentHash").to_ascii_lowercase();
                    if h.len() == 64
                        && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    {
                        h
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                out.push((
                    CloudFile {
                        id,
                        name: string(item, "name"),
                        path: parent_id.into(),
                        is_dir,
                        size: if is_dir { 0 } else { number(item, "size") },
                        modified_at: updated,
                        thumbnail: None,
                        source_id: None,
                    },
                    hash,
                    modified,
                ));
                if out.len() > 100_000 {
                    return Err("单次跨对象库操作超出 100,000 项，请分批处理".into());
                }
            }
            match optional(string(data, "nextPageCursor")) {
                Some(next) if seen_cursors.insert(next.clone()) => cursor = next,
                Some(_) => return Err("云盘目录分页异常".into()),
                None => break,
            }
        }
        Ok(out)
    }

    pub async fn materialize_dedup_file(
        &self,
        parent_id: &str,
        name: &str,
        size: u64,
        sha256: &str,
        source_file_id: Option<&str>,
    ) -> Result<String, String> {
        validate_name(name)?;
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("对象摘要无效".into());
        }
        let cfg = self.account_config().await;
        if cfg.space != "personal_new" {
            return Err("仅支持向个人云目录导出对象库文件".into());
        }
        let part_size = 100 * 1024 * 1024u64;
        let count = size.max(1).div_ceil(part_size);
        let part_infos: Vec<Value> = (1..=count.min(100))
            .map(|part| part_info(size, part_size, part))
            .collect();
        let value = self
            .modern_post(
                "/file/create",
                &json!({
                    "contentHash": sha256.to_ascii_uppercase(),
                    "contentHashAlgorithm": "SHA256",
                    "contentType": "application/octet-stream",
                    "parallelUpload": false,
                    "partInfos": part_infos,
                    "size": size,
                    "parentFileId": parent_id,
                    "name": name,
                    "type": "file",
                    "fileRenameMode": "auto_rename"
                }),
                false,
            )
            .await?;
        let data = &value["data"];
        let created_id = string(data, "fileId");
        if created_id.is_empty() {
            return Err("云盘未返回新文件标识".into());
        }
        let returned_name = optional(string(data, "fileName"))
            .or_else(|| optional(string(data, "name")))
            .unwrap_or_else(|| name.into());
        let rapid = match data.get("partInfos") {
            Some(Value::Null) => true,
            Some(Value::Array(parts)) if !parts.is_empty() => false,
            Some(Value::Array(_)) | None
                if data["rapidUpload"] == true
                    || data["exist"] == true
                    || data["fileExist"] == true =>
            {
                true
            }
            _ => false,
        };
        if rapid {
            if returned_name != name {
                let _ = self.remove_personal_ids(&[created_id]).await;
                return Err(format!("目标目录已存在同名文件：{name}"));
            }
            return Ok(created_id);
        }
        if size == 0 {
            let plan = modern_upload_plan(data, name, 0, part_size, sha256, None)?;
            if returned_name != name {
                let _ = self.remove_personal_ids(&[plan.file_id]).await;
                return Err(format!("目标目录已存在同名文件：{name}"));
            }
            if let Some(first) = plan.parts.first() {
                self.upload_part(first, reqwest::Body::from(Vec::<u8>::new()))
                    .await
                    .map_err(|_| "空文件提交失败".to_string())?;
                self.complete_upload(&plan).await?;
                return Ok(plan.file_id);
            }
        }
        let _ = self.remove_personal_ids(&[created_id]).await;
        let src_id = source_file_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("云端未能秒传文件 {name}，且缺少原对象引用"))?;
        let before: HashSet<String> = self
            .list_personal_entries(parent_id)
            .await?
            .into_iter()
            .map(|(f, _, _)| f.id)
            .collect();
        self.modern_post(
            "/file/batchCopy",
            &json!({"fileIds": [src_id], "toParentFileId": parent_id}),
            false,
        )
        .await?;
        let mut copied: Option<(String, String)> = None;
        for (f, h, _) in self.list_personal_entries(parent_id).await? {
            if !f.is_dir && !before.contains(&f.id) && f.size == size && (h.is_empty() || h == sha256) {
                copied = Some((f.id, f.name));
                break;
            }
        }
        let (copied_id, copied_name) =
            copied.ok_or_else(|| format!("云端复制对象 {name} 后未找到目标项"))?;
        if copied_name != name {
            self.modern_post(
                "/file/update",
                &json!({"fileId": copied_id, "name": name, "description": ""}),
                false,
            )
            .await?;
        }
        Ok(copied_id)
    }
    pub async fn quota(&self) -> Result<Quota, String> {
        let cfg = self.account_config().await;
        if cfg.user_domain_id.is_empty() {
            return Err("容量查询需要 userDomainId（ud_id），可通过密码登录或导入凭据获得".into());
        }
        let value = self
            .signed_post(
                "https://user-njs.yun.139.com/user/disk/quota/detail",
                &json!({"userDomainId":cfg.user_domain_id}),
                false,
            )
            .await?;
        let total = number(&value["data"], "diskSize").saturating_mul(1024 * 1024);
        let available = number(&value["data"], "freeDiskSize").saturating_mul(1024 * 1024);
        Ok(Quota {
            total,
            available,
            used: total.saturating_sub(available),
        })
    }
    pub async fn prepare_upload(
        &self,
        parent: &CloudFile,
        name: &str,
        size: u64,
        sha256: &str,
    ) -> Result<UploadPlan, String> {
        if sha256.len() != 64 || !sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("上传需要完整文件的 SHA256 校验值".into());
        }
        validate_name(name)?;
        if !parent.is_dir {
            return Err("上传目标需要为文件夹".into());
        }
        let cfg = self.account_config().await;
        if cfg.space == "share" {
            return Err("分享空间为只读".into());
        }
        let part_size = if cfg.custom_upload_part_size > 0 {
            cfg.custom_upload_part_size
        } else if size / (1024 * 1024 * 1024) > 30 {
            512 * 1024 * 1024
        } else {
            100 * 1024 * 1024
        };
        if part_size == 0 {
            return Err("分片大小必须大于零".into());
        }
        let count = size.max(1).div_ceil(part_size);
        if count > 100_000 {
            return Err("上传分片过多，请增大分片大小".into());
        }
        let dynamic = cfg.space == "group";
        if dynamic && cfg.cloud_id.is_empty() {
            return Err("群组上传需要 cloudId".into());
        }
        let part_infos: Vec<Value> = (1..=count.min(100))
            .map(|part| part_info(size, part_size, part))
            .collect();
        let mut body = json!({"contentHash":sha256,"contentHashAlgorithm":"SHA256","contentType":"application/octet-stream","parallelUpload":false,"partInfos":part_infos,"size":size,"parentFileId":parent.id,"name":name,"type":"file","fileRenameMode":"auto_rename"});
        if dynamic {
            body["groupId"] = json!(cfg.cloud_id);
            body["groupType"] = json!(2);
            body["catalogType"] = json!(3);
            body["seqNo"] = json!(nonce(32));
        }
        let value = self
            .modern_post(
                if dynamic {
                    "/dynamic/file/create"
                } else {
                    "/file/create"
                },
                &body,
                dynamic,
            )
            .await?;
        modern_upload_plan(
            &value["data"],
            name,
            size,
            part_size,
            sha256,
            dynamic.then(|| cfg.cloud_id),
        )
    }
    pub async fn refresh_upload_parts(
        &self,
        plan: &UploadPlan,
        start_number: u64,
        count: usize,
    ) -> Result<Vec<UploadPart>, String> {
        let total = plan.size.max(1).div_ceil(plan.part_size);
        if start_number == 0 || start_number > total {
            return Err("上传分片序号无效".into());
        }
        let count = count.min(100).min((total - start_number + 1) as usize);
        let infos: Vec<Value> = (start_number..start_number + count as u64)
            .map(|part| part_info(plan.size, plan.part_size, part))
            .collect();
        let mut body = json!({"fileId":plan.file_id,"uploadId":plan.upload_id,"partInfos":infos,"commonAccountInfo":self.common().await?});
        if let Some(group) = &plan.group_id {
            body["groupId"] = json!(group);
        }
        let value = self
            .modern_post(
                if plan.group_id.is_some() {
                    "/dynamic/file/getUploadUrl"
                } else {
                    "/file/getUploadUrl"
                },
                &body,
                plan.group_id.is_some(),
            )
            .await?;
        parse_upload_parts(&value["data"], plan)
    }
    pub async fn upload_part(&self, part: &UploadPart, body: reqwest::Body) -> Result<(), UploadPartError> {
        let response = self
            .transfer_http
            .put(&part.url)
            .header("Content-Length", part.size)
            .header("Content-Type", "application/octet-stream")
            .header("Origin", "https://yun.139.com")
            .header("Referer", "https://yun.139.com/")
            .body(body)
            .send()
            .await
            .map_err(|e| UploadPartError::Transport(format!("分片上传失败：{}", e.without_url())))?;
        if response.status().as_u16() != 200 {
            return Err(UploadPartError::Rejected(response.status()));
        }
        Ok(())
    }
    pub async fn complete_upload(&self, plan: &UploadPlan) -> Result<(), String> {
        if plan.rapid || plan.existing {
            return Ok(());
        }
        let mut body = json!({"contentHash":plan.content_hash,"contentHashAlgorithm":"SHA256","fileId":plan.file_id,"uploadId":plan.upload_id});
        if let Some(group) = &plan.group_id {
            body["groupId"] = json!(group);
        }
        self.modern_post(
            if plan.group_id.is_some() {
                "/dynamic/file/complete"
            } else {
                "/file/complete"
            },
            &body,
            plan.group_id.is_some(),
        )
        .await?;
        Ok(())
    }

    async fn share_directory(&self, reference: &ShareRef) -> Result<Value, String> {
        self.share_post(
            "/richlifeApp/devapp/IOutLink/getOutLinkInfoV6",
            &json!({"getOutLinkInfoReq":{
                "account": self.account().await?,
                "linkID": reference.link_id,
                "passwd": reference.password,
                "pCaID": reference.node_id
            }}),
        ).await
    }

    pub async fn share_title(&self) -> Result<String, String> {
        let config = self.account_config().await;
        let reference = parse_share_entries(&config.link_id)
            .into_iter().next().ok_or("请填写有效的分享链接")?;
        let value = self.share_directory(&reference).await?;
        Ok(optional(string(&value["data"], "lkName")).unwrap_or(reference.link_id))
    }

    async fn share_list(&self, parent: &CloudFile) -> Result<FilePage, String> {
        let cfg = self.account_config().await;
        let refs = if parent.id.starts_with("shares:") {
            decode_share_refs(&parent.id)?
        } else {
            let mut refs = parse_share_entries(&cfg.link_id);
            if parent.id != "root" {
                for reference in &mut refs {
                    reference.node_id = parent.id.clone();
                }
            }
            refs
        };
        if refs.len() > 100 {
            return Err("单个视图最多合并 100 个分享，请分组浏览".into());
        }
        let mut files: Vec<CloudFile> = Vec::new();
        let mut indices: HashMap<String, usize> = HashMap::new();
        let mut first_error = None;
        let mut successes = 0;
        for reference in refs {
            let result = self.share_directory(&reference).await;
            let value = match result {
                Ok(v) => {
                    successes += 1;
                    v
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                    continue;
                }
            };
            for (key, dir) in [("caLst", true), ("coLst", false)] {
                for item in array(&value["data"], key) {
                    if files.len() >= 100_000 {
                        return Err("分享目录超出 100,000 项，请分组浏览".into());
                    }
                    let name = string(item, if dir { "caName" } else { "coName" });
                    let node_id = share_node_id(item, dir)?;
                    let new_ref = ShareRef {
                        node_id: node_id.clone(),
                        ..reference.clone()
                    };
                    let file = CloudFile {
                        id: if parent.source_id.is_some() { node_id } else { encode_share_refs(&[new_ref]) },
                        name: name.clone(),
                        path: parent.id.clone(),
                        is_dir: dir,
                        size: if dir { 0 } else { number(item, "coSize") },
                        modified_at: string(item, "udTime"),
                        thumbnail: None,
                        source_id: parent.source_id.clone(),
                    };
                    if let Some(index) = indices.get(&name).copied().filter(|_| parent.source_id.is_none()) {
                        if dir && files[index].is_dir && parent.source_id.is_none() {
                            let mut merged = decode_share_refs(&files[index].id)?;
                            merged.extend(decode_share_refs(&file.id)?);
                            files[index].id = encode_share_refs(&merged);
                        }
                    } else {
                        indices.insert(name, files.len());
                        files.push(file);
                    }
                }
            }
        }
        if successes == 0 {
            if let Some(error) = first_error {
                return Err(error);
            }
        }
        Ok(FilePage {
            files,
            next_cursor: None,
        })
    }
    async fn share_link(&self, file: &CloudFile, preview: bool) -> Result<String, String> {
        let refs = if file.id.starts_with("shares:") {
            decode_share_refs(&file.id)?
        } else {
            let cfg = self.account_config().await;
            let mut refs = parse_share_entries(&cfg.link_id);
            for reference in &mut refs {
                reference.node_id = file.id.clone();
            }
            refs
        };
        let reference = refs.into_iter().next().ok_or("分享文件引用无效")?;
        let account = self.account().await?;
        let value=self.share_post("/richlifeApp/devapp/IOutLink/getContentInfoFromOutLink",&json!({"getContentInfoFromOutLinkReq":{"contentId":reference.node_id,"linkID":reference.link_id,"passwd":reference.password,"account":account}})).await?;
        let present = string(&value["data"]["contentInfo"], "presentURL");
        if preview || account.is_empty() {
            return valid_download_url(present).map_err(|_| {
                if account.is_empty() {
                    "分享下载需要登录账号，且该文件没有公开预览".into()
                } else {
                    "此文件没有可用预览".into()
                }
            });
        }
        let value=self.share_post("/richlifeApp/devapp/IOutLink/dlFromOutLinkV3",&json!({"dlFromOutLinkReqV3":{"account":account,"linkID":reference.link_id,"passwd":reference.password,"coIDLst":{"item":[reference.node_id]}}})).await?;
        let data = &value["data"];
        for value in [
            string(&data["extInfo"], "cdnDownloadUrl"),
            string(data, "redrUrl"),
            string(data, "downloadURL"),
        ] {
            if !value.is_empty() {
                return valid_download_url(value);
            }
        }
        Err("分享下载未返回可用地址".into())
    }
}

fn legacy_files(
    data: &Value,
    catalog_key: &str,
    content_key: &str,
    parent: &str,
) -> Vec<CloudFile> {
    let mut files = Vec::new();
    for item in array(data, catalog_key) {
        files.push(CloudFile {
            id: string(item, "catalogID"),
            name: string(item, "catalogName"),
            path: optional(string(item, "path")).unwrap_or_else(|| parent.into()),
            is_dir: true,
            size: 0,
            modified_at: string(item, "updateTime"),
            thumbnail: None,
            source_id: None,
        });
    }
    for item in array(data, content_key) {
        files.push(CloudFile {
            id: string(item, "contentID"),
            name: string(item, "contentName"),
            path: parent.into(),
            is_dir: false,
            size: number(item, "contentSize"),
            modified_at: string(item, "updateTime"),
            thumbnail: optional(string(item, "thumbnailURL")),
            source_id: None,
        });
    }
    files
}
fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return Err("文件名不能为空，也不能包含路径分隔符或控制字符".into());
    }
    Ok(())
}
fn parse_cloud_millis(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        let ms = dt.timestamp_millis();
        return axuanstor_sdk::repository::valid_file_modified_at(ms).then_some(ms);
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        let tz = FixedOffset::east_opt(8 * 3600)?;
        let dt = naive.and_local_timezone(tz).single()?;
        let ms = dt.timestamp_millis();
        return axuanstor_sdk::repository::valid_file_modified_at(ms).then_some(ms);
    }
    None
}
fn valid_download_url(value: String) -> Result<String, String> {
    let url = url::Url::parse(&value).map_err(|_| "云盘未返回有效的下载地址")?;
    if !["https", "http"].contains(&url.scheme()) || url.host_str().is_none() {
        return Err("下载地址无效".into());
    }
    Ok(value)
}
fn part_info(size: u64, part_size: u64, part: u64) -> Value {
    let offset = (part - 1) * part_size;
    json!({"partNumber":part,"partSize":size.saturating_sub(offset).min(part_size),"parallelHashCtx":{"partOffset":offset}})
}
fn modern_upload_plan(
    data: &Value,
    name: &str,
    size: u64,
    part_size: u64,
    sha256: &str,
    group_id: Option<String>,
) -> Result<UploadPlan, String> {
    if !data.is_object() || part_size == 0 {
        return Err("云盘上传响应格式无效".into());
    }
    let file_id = string(data, "fileId");
    if file_id.trim().is_empty() {
        return Err("云盘上传响应缺少文件标识".into());
    }
    let existing = data["exist"] == true;
    // Some deployments report a rapid upload with an explicit null partInfos.
    // Missing data must never be interpreted as a successful upload. When the
    // server supplies part URLs, send those parts even if rapidUpload is true.
    let rapid = match data.get("partInfos") {
        Some(Value::Null) => true,
        Some(Value::Array(parts)) if !parts.is_empty() => false,
        Some(Value::Array(_)) | None if data["rapidUpload"] == true || existing => true,
        Some(Value::Array(_)) | None => return Err("云盘未返回上传分片或明确的快传结果".into()),
        Some(_) => return Err("云盘上传分片响应格式无效".into()),
    };
    let upload_id = string(data, "uploadId");
    if sha256.is_empty() && (rapid || existing) {
        return Err("未计算哈希的任务需要实际传输数据；请开启哈希后重新添加".into());
    }
    if !rapid && !existing && upload_id.trim().is_empty() {
        return Err("云盘未返回完整的上传会话".into());
    }
    let mut plan = UploadPlan {
        file_id,
        upload_id,
        name: optional(string(data, "fileName")).unwrap_or_else(|| name.into()),
        size,
        part_size,
        parts: vec![],
        rapid,
        existing,
        content_hash: sha256.into(),
        group_id,
    };
    if !rapid && !existing {
        plan.parts = parse_upload_parts(data, &plan)?;
    }
    Ok(plan)
}
fn parse_upload_parts(data: &Value, plan: &UploadPlan) -> Result<Vec<UploadPart>, String> {
    let mut out = Vec::new();
    let total = plan.size.max(1).div_ceil(plan.part_size);
    let mut numbers = HashSet::new();
    for item in array(data, "partInfos") {
        let part = number(item, "partNumber");
        if part == 0 || part > total || !numbers.insert(part) {
            return Err("云盘返回了无效或重复的分片编号".into());
        }
        let offset = (part - 1)
            .checked_mul(plan.part_size)
            .ok_or("上传分片偏移溢出")?;
        out.push(UploadPart {
            part_number: part,
            offset,
            size: plan.size.saturating_sub(offset).min(plan.part_size),
            url: valid_download_url(string(item, "uploadUrl"))?,
        });
    }
    out.sort_by_key(|p| p.part_number);
    Ok(out)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShareRef {
    link_id: String,
    password: String,
    node_id: String,
}
fn encode_share_refs(refs: &[ShareRef]) -> String {
    format!(
        "shares:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(refs).unwrap_or_default())
    )
}
fn decode_share_refs(id: &str) -> Result<Vec<ShareRef>, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(id.strip_prefix("shares:").ok_or("分享文件引用无效")?)
        .map_err(|_| "分享引用编码无效")?;
    serde_json::from_slice(&bytes).map_err(|_| "分享引用内容无效".into())
}
fn share_node_id(item: &Value, dir: bool) -> Result<String, String> {
    let keys = if dir { ["caID", "caId"] } else { ["coID", "coId"] };
    keys.into_iter()
        .find_map(|key| optional(string(item, key)))
        .ok_or_else(|| "云盘未返回分享项目的 ID".into())
}

pub fn normalize_share_input(input: &str) -> Result<String, String> {
    let code = input.lines().find_map(|line| {
        let line = line.trim().strip_prefix("提取码")?;
        line.trim_start_matches([':', '：', ' ']).split_whitespace().next()
    });
    let link = input.find("https://").or_else(|| input.find("http://"))
        .and_then(|index| input[index..].split_whitespace().next())
        .or_else(|| input.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with("提取码")))
        .unwrap_or_default();
    let mut refs = parse_share_entries(link);
    if refs.len() != 1 {
        return Err("请填写一个有效的移动云盘分享链接或分享 ID".into());
    }
    let reference = refs.pop().unwrap();
    if !reference.link_id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err("分享 ID 格式无效".into());
    }
    let password = code.unwrap_or(&reference.password);
    if !password.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return Err("提取码格式无效".into());
    }
    Ok(format!("{}#{password}", reference.link_id))
}

pub fn saved_share_inputs(input: &str) -> Vec<String> {
    parse_share_entries(input).into_iter()
        .filter_map(|reference| normalize_share_input(&format!("{}#{}", reference.link_id, reference.password)).ok())
        .collect()
}

fn parse_share_entries(input: &str) -> Vec<ShareRef> {
    input
        .split([',', ';', '\n', '\r'])
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let (id, password) = if item.starts_with("https://") || item.starts_with("http://") {
                let url = url::Url::parse(item).ok()?;
                if !(url.host_str()? == "yun.139.com" || url.host_str()?.ends_with(".yun.139.com"))
                {
                    return None;
                }
                let fragment = url.fragment().unwrap_or_default();
                let route = fragment.split('?').next().unwrap_or_default();
                let fragment_id = route
                    .starts_with('/')
                    .then(|| route.rsplit('/').find(|s| !s.is_empty()).map(str::to_owned))
                    .flatten();
                let id = url
                    .query_pairs()
                    .find(|(k, _)| k == "linkID" || k == "linkId")
                    .map(|(_, v)| v.into_owned())
                    .or(fragment_id)
                    .or_else(|| {
                        url.path_segments()
                            .and_then(|segments| segments.filter(|s| !s.is_empty()).next_back())
                            .filter(|s| *s != "shareweb")
                            .map(str::to_owned)
                    })?;
                let password = url
                    .query_pairs()
                    .find(|(k, _)| ["passwd", "password", "code"].contains(&k.as_ref()))
                    .map(|(_, v)| v.into_owned())
                    .or_else(|| {
                        fragment.split_once('?').and_then(|(_, q)| {
                            url::form_urlencoded::parse(q.as_bytes())
                                .find(|(k, _)| ["passwd", "password", "code"].contains(&k.as_ref()))
                                .map(|(_, v)| v.into_owned())
                        })
                    })
                    .unwrap_or_else(|| {
                        if fragment.contains('/') {
                            String::new()
                        } else {
                            fragment.to_owned()
                        }
                    });
                (id, password)
            } else {
                let (id, password) = item.split_once('#').unwrap_or((item, ""));
                (id.to_owned(), password.to_owned())
            };
            (!id.is_empty()).then(|| ShareRef {
                link_id: id,
                password: password.trim().into(),
                node_id: "root".into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn business_response_requires_positive_success() {
        for value in [
            Value::Null,
            json!({}),
            json!("ok"),
            json!({"success":null}),
            json!({"success":"true"}),
            json!({"code":"0","data":{}}),
            json!({"data":{}}),
        ] {
            assert!(validate_response(&value).is_err());
        }
        for value in [
            json!({"success":true}),
            json!({"result":{"resultCode":"0"}}),
            json!({"success":false,"result":{"resultCode":0}}),
            json!({"data":{"result":{"resultCode":"0"}}}),
        ] {
            assert!(validate_response(&value).is_ok());
        }
    }
    #[test]
    fn business_result_errors_override_a_success_envelope() {
        for value in [
            json!({"success":false,"message":"fixture failure","code":"403"}),
            json!({"success":true,"result":{"resultCode":"1"}}),
            json!({"success":true,"data":{"result":{"resultCode":1}}}),
        ] {
            assert!(validate_response(&value).is_err());
        }
        // The batch endpoint has an independent root success result.
        assert!(validate_response(&json!({"success":false,"result":{"resultCode":"0"}})).is_ok());
    }
    #[test]
    fn retired_space_requests_are_rejected_before_authentication() {
        for space in ["personal", "family"] {
            assert!(CloudClient::new(AccountConfig {
                space: space.into(),
                ..Default::default()
            })
            .is_err());
        }
    }
    #[test]
    fn saved_space_migration_preserves_credentials_and_resets_directory_ids() {
        for space in ["personal", "family", "group", "share", "personal_new"] {
            let mut config = AccountConfig {
                space: space.into(),
                authorization: "fixture-auth".into(),
                root_id: "old-root".into(),
                cloud_id: "old-cloud".into(),
                link_id: "old-link".into(),
                ..Default::default()
            };
            config.migrate_saved_space();
            assert_eq!(config.authorization, "fixture-auth");
            if ["personal", "family"].contains(&space) {
                assert_eq!(config.space, "personal_new");
                assert!(
                    config.root_id.is_empty()
                        && config.cloud_id.is_empty()
                        && config.link_id.is_empty()
                );
            } else {
                assert_eq!(config.space, space);
                assert_eq!(config.root_id, "old-root");
                assert_eq!(config.cloud_id, "old-cloud");
                assert_eq!(config.link_id, "old-link");
            }
        }
    }
    #[test]
    fn component_matches_javascript() {
        assert_eq!(encode_uri_component("a b!中'()*~"), "a%20b!%E4%B8%AD'()*~");
    }
    #[test]
    fn signing_order_independent() {
        assert_eq!(
            sign(
                "{\"a\":1,\"b\":2}",
                "2025-01-01 00:00:00",
                "abcdefghijklmnop"
            ),
            sign(
                "{\"b\":2,\"a\":1}",
                "2025-01-01 00:00:00",
                "abcdefghijklmnop"
            )
        );
    }
    #[test]
    fn signature_matches_go_driver_fixture() {
        // Generated independently using the original driver's Go algorithm.
        assert_eq!(
            sign(
                r#"{"folder":"中文 空格!'()*~","size":5497558138880}"#,
                "2026-10-05 12:34:56",
                "abcdefghijklmnop"
            ),
            "E678E904C58379B81A073915F30C0281"
        );
    }
    #[test]
    fn tb_parts_preserve_u64() {
        let size = 5u64 * 1024 * 1024 * 1024 * 1024;
        let v = part_info(size, 512 * 1024 * 1024, 10000);
        assert_eq!(
            v["parallelHashCtx"]["partOffset"],
            json!(9999u64 * 512 * 1024 * 1024)
        );
    }
    fn plan_from_fixture(data: Value) -> Result<UploadPlan, String> {
        modern_upload_plan(&data, "fixture.bin", 16, 8, &"A".repeat(64), None)
    }
    #[test]
    fn malformed_upload_response_cannot_complete_a_transfer() {
        for data in [
            Value::Null,
            json!({}),
            json!({"rapidUpload":true}),
            json!({"exist":true}),
            json!({"fileId":" ","partInfos":null}),
            json!({"fileId":"file"}),
            json!({"fileId":"file","uploadId":"session","partInfos":[]}),
            json!({"fileId":"file","partInfos":"invalid"}),
            json!({"fileId":"file","partInfos":[{"partNumber":1,"uploadUrl":"https://storage.example/part"}]}),
        ] {
            assert!(plan_from_fixture(data).is_err());
        }
    }
    #[test]
    fn explicit_rapid_and_existing_uploads_require_a_file_identifier() {
        for data in [
            json!({"fileId":"file","rapidUpload":true}),
            json!({"fileId":"file","partInfos":null}),
            json!({"fileId":"file","rapidUpload":true,"partInfos":[]}),
            json!({"fileId":"file","exist":true}),
        ] {
            let plan = plan_from_fixture(data).unwrap();
            assert!(plan.rapid || plan.existing);
            assert_eq!(plan.file_id, "file");
            assert!(plan.parts.is_empty());
        }
    }
    #[test]
    fn returned_upload_parts_take_precedence_over_rapid_flag() {
        let plan = plan_from_fixture(json!({
            "fileId":"file","uploadId":"session","rapidUpload":true,
            "partInfos":[
                {"partNumber":2,"uploadUrl":"https://storage.example/part-2"},
                {"partNumber":1,"uploadUrl":"https://storage.example/part-1"}
            ]
        }))
        .unwrap();
        assert!(!plan.rapid);
        assert_eq!(plan.parts.len(), 2);
        assert_eq!(
            (
                plan.parts[0].part_number,
                plan.parts[0].offset,
                plan.parts[0].size
            ),
            (1, 0, 8)
        );
        assert_eq!(
            (
                plan.parts[1].part_number,
                plan.parts[1].offset,
                plan.parts[1].size
            ),
            (2, 8, 8)
        );
    }
    #[test]
    fn share_urls_and_passwords() {
        let refs = parse_share_entries(
            "https://yun.139.com/shareweb/#/w/i/example?code=5678,abc#1234\ndef",
        );
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].link_id, "example");
        assert_eq!(refs[0].password, "5678");
        assert_eq!(refs[1].password, "1234");
    }

    #[test]
    fn share_inputs_keep_extraction_code_and_provider_node_ids() {
        assert_eq!(
            normalize_share_input("分享文件\n链接：https://yun.139.com/shareweb/#/w/i/example\n提取码： aB12\n复制本段信息").unwrap(),
            "example#aB12",
        );
        assert_eq!(normalize_share_input("提取码：1234\nexample").unwrap(), "example#1234");
        assert_eq!(share_node_id(&json!({"caID":"directory"}), true).unwrap(), "directory");
        assert_eq!(share_node_id(&json!({"coID":"file"}), false).unwrap(), "file");
        assert!(share_node_id(&json!({"caName":"目录"}), true).is_err());
    }

    #[tokio::test]
    #[ignore = "Requires CMCLOUD_TEST_SHARE and access to the 139 share API"]
    async fn live_share_lists_shared_folder() {
        let input = std::env::var("CMCLOUD_TEST_SHARE").expect("Set CMCLOUD_TEST_SHARE");
        let cloud = CloudClient::new(AccountConfig {
            space: "share".into(),
            root_id: "root".into(),
            link_id: normalize_share_input(&input).unwrap(),
            ..Default::default()
        }).unwrap();
        let mut root = cloud.root().await;
        root.source_id = Some("test-source".into());
        let title = cloud.share_title().await.unwrap();
        let page = cloud.list_page(&root, None).await.unwrap();
        let folder = page.files.iter().find(|file| file.is_dir).expect("shared folder");
        let nested = cloud.list_page(folder, None).await.unwrap();
        assert!(!nested.files.is_empty());
        assert!(nested.files.iter().all(|item| !item.id.is_empty() && item.id != folder.id));
        println!("{} · {} root entries, {} nested entries", title, page.files.len(), nested.files.len());
    }
}

#[cfg(all(test, target_os = "windows"))]
mod live_upload_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Requires CMCLOUD_LIVE_VAULT; creates and removes a uniquely named test file"]
    async fn personal_upload_roundtrip() {
        let vault_path = std::env::var("CMCLOUD_LIVE_VAULT").expect("Set encrypted vault path");
        let directory = std::env::temp_dir().join(format!("cmcloud-live-{}", uuid::Uuid::new_v4()));
        let store = crate::store::Store::new(&directory).unwrap();
        let plain = store.decrypt(&std::fs::read(vault_path).unwrap()).unwrap();
        let value: Value = serde_json::from_slice(&plain).unwrap();
        let config: AccountConfig = if value.get("accounts").is_some() {
            let id = value["activeId"].as_str();
            serde_json::from_value(
                value["accounts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["id"].as_str() == id)
                    .unwrap()
                    .clone(),
            )
            .unwrap()
        } else {
            serde_json::from_value(value).unwrap()
        };
        assert_eq!(
            config.space, "personal_new",
            "Live test requires personal space"
        );
        let client = CloudClient::new(config).unwrap();
        assert!(
            client.initialize().await.is_ok(),
            "Live authentication failed"
        );
        let parent = CloudFile {
            id: "/".into(),
            is_dir: true,
            ..Default::default()
        };
        let name = format!("cmcloud-upload-check-{}.txt", uuid::Uuid::new_v4());
        let bytes = b"CMCloud++ upload roundtrip\n";
        let result = client
            .prepare_upload(
                &parent,
                &name,
                bytes.len() as u64,
                &hex::encode(sha2::Sha256::digest(bytes)).to_uppercase(),
            )
            .await;
        if let Err(error) = &result {
            // Protocol errors are already redacted by the adapter.
            eprintln!("Upload create error: {error}");
        }
        assert!(result.is_ok(), "Cloud rejected upload create");
        let plan = result.unwrap();
        let remote = CloudFile {
            id: plan.file_id.clone(),
            name: name.clone(),
            ..Default::default()
        };
        let transfer = async {
            assert!(
                !plan.rapid && !plan.existing,
                "Unique test must transfer bytes"
            );
            let part = plan.parts.first().expect("Upload part");
            client
                .upload_part(part, reqwest::Body::from(bytes.as_slice().to_vec()))
                .await?;
            client.complete_upload(&plan).await?;
            let url = client.link(&remote).await?;
            let downloaded = reqwest::Client::new()
                .get(url)
                .send()
                .await
                .map_err(|_| "Readback failed")?
                .bytes()
                .await
                .map_err(|_| "Readback failed")?;
            if downloaded.as_ref() != bytes {
                return Err("Readback differs".to_string());
            }
            Ok::<_, String>(())
        }
        .await;
        let cleanup = client.remove(&remote).await;
        drop(store);
        let _ = std::fs::remove_dir_all(directory);
        assert!(cleanup.is_ok(), "Live test cleanup failed");
        assert!(transfer.is_ok(), "Live upload or readback failed");
    }
}
