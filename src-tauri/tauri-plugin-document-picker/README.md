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

The Rust queue refreshes `begin_transfers_with_status()` every ten seconds while work is live, independently of WebView timers. The bridge carries aggregate byte progress, counts, and upload/download speeds for the ongoing notification. Idle queues call `end_transfers()` and release resources.

Android 14+ prefers a user-initiated data transfer `JobService`, scheduled while the activity is visible, with network constraints and estimated transfer sizes. Older releases, or a rejected job request, use the `dataSync` foreground service. Both paths hold a partial CPU wake lock, high-performance Wi-Fi lock and an Internet network request, and keep the visible activity's screen awake during transfers. The Kotlin watchdog checks a three-minute native lease; locks are released when the queue is idle or the runtime stops. Transient Android network failures continue retrying with capped backoff until recovery or explicit pause/cancel.

Notification and battery-optimization exemption prompts are requested once from the visible app. The Android settings entry can request them again and open the app's system settings for manufacturer-specific background permissions. These are user-approved system settings. The job/service does not resurrect a killed Rust runtime; the persisted queue resumes on the next launch. Force-stop and system health restrictions still apply. Android 15's six-hour dataSync service budget applies to the foreground-service fallback and its timeout is handled safely.

`foreground_status()` returns `{supported, active, notificationPermission, reason}`. Transfer argument fields and service classes are retained by consumer ProGuard rules.

References: [User-initiated data transfer](https://developer.android.com/develop/background-work/background-tasks/uidt), [foreground service timeouts](https://developer.android.com/develop/background-work/services/fgs/timeout), [Doze and App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby).

References: [Android Storage Access Framework](https://developer.android.com/training/data-storage/shared/documents-files),
[AOSP URI grant implementation](https://android.googlesource.com/platform/frameworks/base/+/master/services/core/java/com/android/server/uri/UriGrantsManagerService.java).
