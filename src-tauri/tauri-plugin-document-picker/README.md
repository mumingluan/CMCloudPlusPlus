# CMCloud++ Android document picker

Register the path dependency `tauri-plugin-document-picker = { path = "tauri-plugin-document-picker" }`
and add `.plugin(tauri_plugin_document_picker::init())` to the Tauri builder.

The Rust backend imports `tauri_plugin_document_picker::DocumentPickerExt` and calls:

```rust
let files = app.document_picker().pick_files().await?;
let directory = app.document_picker().pick_directory().await?;
let page = app.document_picker().list_children(&tree_uri, parent_uri.as_deref(), cursor.as_deref()).await?;
app.document_picker().close_scan(&cursor).await?;
let background = app.document_picker().begin_transfers().await?;
let background = app.document_picker().foreground_status().await?;
app.document_picker().end_transfers().await?;
```

Android uses `ACTION_OPEN_DOCUMENT` and `ACTION_OPEN_DOCUMENT_TREE` and retains read-only grants.
The result contains provider display names, optional 64-bit sizes, and content URIs; it never
copies selected file bytes. File selections are limited to 64 and the conservative persisted
grant budget to 128 to protect queued work on older supported Android versions. Directory
selection retains one tree grant for all descendants, making it the intended bulk-upload path.

Child metadata is returned in pages of 200 from a live provider cursor on a worker thread.
Close `next_cursor` when cancelling traversal. Cursors close when exhausted, idle for two minutes
(checked on the next scan), or when the plugin is destroyed. Caller enforces a total traversal
limit, handles cycles/deduplication, and persists URI/name/size in its upload queue. A user moving
or deleting a document, revoking a grant, or removing its provider can still require reselection.

Desktop and iOS callers should use the existing dialog plugin. This crate adds no Android
storage permissions and exposes no JavaScript commands.

## Android transfer lifetime

`begin_transfers()` starts a `dataSync` foreground service with an ongoing transfer notification
and a partial wake lock. Call it on the user's queue action, refresh every 30–60 seconds while
work is active, and call `end_transfers()` when idle. Both the lease and wake-lock timeout are
two minutes, so an absent Rust runtime cannot leave an unlimited wake lock. The service does
not restart a killed Rust runtime; the persistent queue provides recovery on the next launch.

`foreground_status()` returns `{supported, active, notificationPermission, reason}`. The plugin
declares Android 13 notification permission but does not prompt automatically or block transfers
if it is denied; the foreground service can still be visible in the system's active-app UI.
Starting from the background can be restricted. Android 15 and newer limit `dataSync` foreground
services to six background hours in a 24-hour period; `onTimeout()` stops the service safely and
sets a status reason. Keep the application foregrounded or reopen it to continue after OS limits.
Persisted URI grants also cannot survive document deletion, grant revocation, or provider removal.

Reference: [Android foreground service timeouts](https://developer.android.com/develop/background-work/services/fgs/timeout).

References: [Android Storage Access Framework](https://developer.android.com/training/data-storage/shared/documents-files),
[AOSP URI grant implementation](https://android.googlesource.com/platform/frameworks/base/+/master/services/core/java/com/android/server/uri/UriGrantsManagerService.java).
