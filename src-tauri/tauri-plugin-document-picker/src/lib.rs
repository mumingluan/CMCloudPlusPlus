//! Native Storage Access Framework selection. Public APIs are backend-only;
//! Android document URIs remain durable without staging selected file bytes.

use serde::{Deserialize, Serialize};
#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedDocument {
    pub path: String,
    pub name: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedDirectory {
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEntry {
    pub path: String,
    pub name: String,
    pub size: Option<u64>,
    pub is_directory: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPage {
    pub entries: Vec<DocumentEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForegroundStatus {
    pub supported: bool,
    pub active: bool,
    pub notification_permission: bool,
    #[serde(default)]
    pub user_stopped: bool,
    pub reason: Option<String>,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Deserialize)]
struct FilesResponse {
    files: Vec<PickedDocument>,
}
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Deserialize)]
struct DirectoryResponse {
    directory: Option<PickedDirectory>,
}

pub struct DocumentPicker<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    runtime: std::marker::PhantomData<fn() -> R>,
}

pub trait DocumentPickerExt<R: Runtime> {
    fn document_picker(&self) -> &DocumentPicker<R>;
}

impl<R: Runtime, T: Manager<R>> DocumentPickerExt<R> for T {
    fn document_picker(&self) -> &DocumentPicker<R> {
        self.state::<DocumentPicker<R>>().inner()
    }
}

impl<R: Runtime> DocumentPicker<R> {
    #[cfg(target_os = "android")]
    pub async fn open_media_stream(&self, url: &str, mime: &str) -> Result<(), String> {
        self.call::<serde_json::Value>("openMediaStream", serde_json::json!({ "url": url, "mime": mime }))
            .await
            .map(|_| ())
    }

    pub async fn set_video_immersive(&self, enabled: bool) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            self.call::<serde_json::Value>(
                "setVideoImmersive",
                serde_json::json!({ "enabled": enabled }),
            )
            .await
            .map(|_| ())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = enabled;
            Ok(())
        }
    }

    /// Protect opaque AxuanStor unlock/token bytes with Android Keystore.
    /// Input/output use base64 only across this native bridge, never logging it.
    pub async fn seal_secret(&self, id: &str, data: &str) -> Result<String, String> {
        self.system_secret("sealSecret",id,data).await
    }
    pub async fn unseal_secret(&self, id: &str, data: &str) -> Result<String, String> {
        self.system_secret("unsealSecret",id,data).await
    }
    pub async fn clear_secrets(&self)->Result<(),String>{
        #[cfg(target_os="android")]
        {self.call::<serde_json::Value>("clearSecrets",serde_json::json!({})).await.map(|_|())}
        #[cfg(not(target_os="android"))]
        {Err("此安全存储桥接仅用于 Android".into())}
    }
    async fn system_secret(&self, command: &'static str, id: &str, data: &str) -> Result<String,String> {
        #[cfg(target_os="android")]
        {
            let value:serde_json::Value=self.call(command,serde_json::json!({"id":id,"data":data})).await?;
            value["data"].as_str().map(str::to_owned).ok_or_else(||"系统安全存储返回无效数据".into())
        }
        #[cfg(not(target_os="android"))]
        {let _=(command,id,data);Err("此安全存储桥接仅用于 Android".into())}
    }
    /// Open an app-owned finished download using a narrowly scoped FileProvider.
    pub async fn open_download(&self, path: &str) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            self.call::<serde_json::Value>("openDownload", serde_json::json!({"path":path}))
                .await
                .map(|_| ())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = path;
            Err("此文档打开接口仅支持 Android".into())
        }
    }
    #[cfg(target_os = "android")]
    pub async fn ensure_shared_downloads(&self) -> Result<(), String> {
        self.call::<serde_json::Value>("ensureSharedDownloads", serde_json::json!({}))
            .await
            .map(|_| ())
    }

    #[cfg(target_os = "android")]
    pub async fn prepare_shared_download(
        &self, path: &str, folder: &str, uri: Option<&str>,
    ) -> Result<String, String> {
        let result: serde_json::Value = self.call("prepareSharedDownload",
            serde_json::json!({ "path": path, "folder": folder, "uri": uri })).await?;
        result["uri"].as_str().map(str::to_owned).ok_or_else(|| "公共下载记录无效".into())
    }

    #[cfg(target_os = "android")]
    pub async fn finish_shared_download(&self, path: &str, uri: &str) -> Result<(), String> {
        self.call::<serde_json::Value>("finishSharedDownload", serde_json::json!({ "path": path, "uri": uri }))
            .await
            .map(|_| ())
    }

    #[cfg(target_os = "android")]
    pub async fn discard_shared_download(&self, uri: &str) -> Result<(), String> {
        self.call::<serde_json::Value>("discardSharedDownload", serde_json::json!({ "uri": uri }))
            .await
            .map(|_| ())
    }

    #[cfg(target_os = "android")]
    pub async fn save_shared_download(&self, path: &str) -> Result<bool, String> {
        self.ensure_shared_downloads().await?;
        let uri = self.prepare_shared_download(path, "", None).await?;
        if let Err(error) = self.finish_shared_download(path, &uri).await {
            let _ = self.discard_shared_download(&uri).await;
            return Err(error);
        }
        Ok(true)
    }

    /// Export bytes through the user's chosen document provider in bounded chunks.
    pub async fn export_download(&self, path: &str) -> Result<bool, String> {
        #[cfg(target_os = "android")]
        {
            let result: serde_json::Value = self
                .call("exportDownload", serde_json::json!({"path":path}))
                .await?;
            Ok(result["saved"].as_bool().unwrap_or(false))
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = path;
            Err("此导出接口仅支持 Android".into())
        }
    }
    /// Start or refresh a two-minute foreground transfer lease. Start from a
    /// user-initiated queue action, then refresh every 30–60 seconds while busy.
    pub async fn begin_transfers(&self) -> Result<ForegroundStatus, String> {
        self.begin_transfers_with_status(serde_json::json!({}))
            .await
    }

    pub async fn begin_transfers_with_status(
        &self,
        status: serde_json::Value,
    ) -> Result<ForegroundStatus, String> {
        #[cfg(target_os = "android")]
        {
            self.call("beginTransfers", status).await
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = status;
            self.foreground_status().await
        }
    }

    pub async fn request_transfer_protection(&self) -> Result<ForegroundStatus, String> {
        #[cfg(target_os = "android")]
        {
            self.call("requestTransferProtection", serde_json::json!({}))
                .await
        }
        #[cfg(not(target_os = "android"))]
        {
            self.foreground_status().await
        }
    }

    pub async fn end_transfers(&self) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            self.call::<serde_json::Value>("endTransfers", serde_json::json!({}))
                .await
                .map(|_| ())
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(())
        }
    }

    pub async fn foreground_status(&self) -> Result<ForegroundStatus, String> {
        #[cfg(target_os = "android")]
        {
            self.call("foregroundStatus", serde_json::json!({})).await
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(ForegroundStatus {
                supported: false,
                active: false,
                notification_permission: false,
                user_stopped: false,
                reason: None,
            })
        }
    }

    /// Select up to 64 separate files. Large batches should select a directory
    /// to share one persisted tree grant across all descendants.
    pub async fn pick_files(&self) -> Result<Vec<PickedDocument>, String> {
        #[cfg(target_os = "android")]
        {
            self.call::<FilesResponse>("pickFiles", serde_json::json!({}))
                .await
                .map(|r| r.files)
        }
        #[cfg(not(target_os = "android"))]
        {
            Err("持久文档选择器仅用于 Android；桌面端请使用系统文件选择器。".into())
        }
    }

    pub async fn pick_directory(&self) -> Result<Option<PickedDirectory>, String> {
        #[cfg(target_os = "android")]
        {
            self.call::<DirectoryResponse>("pickDirectory", serde_json::json!({}))
                .await
                .map(|r| r.directory)
        }
        #[cfg(not(target_os = "android"))]
        {
            Err("持久目录选择器仅用于 Android；桌面端请使用系统目录选择器。".into())
        }
    }

    /// Stream at most 200 child metadata records. Pass `next_cursor` to continue
    /// the same provider cursor; close it when cancelling traversal.
    pub async fn list_children(
        &self,
        tree: &str,
        parent: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<DocumentPage, String> {
        #[cfg(target_os = "android")]
        {
            self.call(
                "listChildren",
                serde_json::json!({"tree":tree,"parent":parent,"cursor":cursor}),
            )
            .await
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (tree, parent, cursor);
            Err("此接口仅支持 Android 文档目录。".into())
        }
    }

    pub async fn close_scan(&self, cursor: &str) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            self.call::<serde_json::Value>("closeScan", serde_json::json!({"cursor":cursor}))
                .await
                .map(|_| ())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = cursor;
            Ok(())
        }
    }

    #[cfg(target_os = "android")]
    async fn call<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        command: &'static str,
        payload: serde_json::Value,
    ) -> Result<T, String> {
        let handle = self.handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            handle
                .run_mobile_plugin(command, payload)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "Android 文档服务任务已终止。".to_string())?
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("document-picker")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let picker = DocumentPicker {
                handle: api.register_android_plugin(
                    "com.cmcloud.documentpicker",
                    "DocumentPickerPlugin",
                )?,
            };
            #[cfg(not(target_os = "android"))]
            let picker = {
                let _ = api;
                DocumentPicker {
                    runtime: std::marker::PhantomData::<fn() -> R>,
                }
            };
            app.manage(picker);
            Ok(())
        })
        .build()
}
