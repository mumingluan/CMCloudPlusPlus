package com.cmcloud.documentpicker

import android.app.Activity
import android.Manifest
import android.content.Intent
import android.content.ContentValues
import android.content.ComponentName
import android.content.Context
import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import android.content.pm.PackageManager
import android.database.Cursor
import android.os.Build
import android.os.Handler
import android.os.Environment
import android.os.Looper
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import java.io.File
import java.io.FileOutputStream
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

@InvokeArg
class ChildrenArgs {
    lateinit var tree: String
    var parent: String? = null
    var cursor: String? = null
}

@InvokeArg
class CloseScanArgs { lateinit var cursor: String }
@InvokeArg
class DownloadArgs { lateinit var path: String }
@InvokeArg
class SharedDownloadArgs {
    lateinit var path: String
    var uri: String? = null
    var folder: String = ""
}
@InvokeArg
class SystemSecretArgs { lateinit var id: String; lateinit var data: String }
@InvokeArg
class VideoImmersiveArgs { var enabled: Boolean = false }
@InvokeArg
class MediaStreamArgs { lateinit var url: String; lateinit var mime: String }

private class Scan(
    val tree: Uri,
    val parent: Uri,
    val cursor: Cursor,
    var touched: Long = System.nanoTime(),
)

/** SAF metadata and persisted read grants, without staging document bytes. */
@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.WRITE_EXTERNAL_STORAGE], alias = "legacyDownloads")])
class DocumentPickerPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun openMediaStream(invoke: Invoke) {
        val args = invoke.parseArgs(MediaStreamArgs::class.java)
        val uri = Uri.parse(args.url)
        if (uri.scheme != "http" || uri.host != "127.0.0.1" || uri.port <= 0 ||
            !(args.mime.startsWith("video/") || args.mime.startsWith("audio/"))) {
            invoke.reject("无效的本机播放地址")
            return
        }
        activity.runOnUiThread {
            try {
                val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, args.mime)
                activity.startActivity(Intent.createChooser(intent, "选择播放器"))
                invoke.resolve(JSObject())
            } catch (_: Exception) {
                invoke.reject("无法打开外部播放器，请下载后打开。")
            }
        }
    }

    @Command
    fun setVideoImmersive(invoke: Invoke) {
        val enabled = invoke.parseArgs(VideoImmersiveArgs::class.java).enabled
        activity.runOnUiThread {
            val window = activity.window
            val controller = WindowCompat.getInsetsController(window, window.decorView)
            if (enabled) {
                controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                controller.hide(WindowInsetsCompat.Type.systemBars())
            } else {
                controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_DEFAULT
                controller.show(WindowInsetsCompat.Type.systemBars())
            }
            invoke.resolve(JSObject())
        }
    }

    @Command
    fun sealSecret(invoke: Invoke) = systemSecret(invoke, false)

    @Command
    fun unsealSecret(invoke: Invoke) = systemSecret(invoke, true)
    @Command
    fun clearSecrets(invoke: Invoke) {
        worker.execute {
            try { SystemSecrets.clear(); invoke.resolve(JSObject()) }
            catch (_: Exception) { invoke.reject("系统安全存储密钥未能移除，请重试") }
        }
    }

    private fun systemSecret(invoke: Invoke, decrypt: Boolean) {
        val args = invoke.parseArgs(SystemSecretArgs::class.java)
        if (args.data.length > 180000 || !args.id.matches(Regex("[A-Za-z0-9._:-]{1,128}"))) {
            invoke.reject("安全存储请求无效")
            return
        }
        worker.execute {
            var bytes: ByteArray? = null
            var transformed: ByteArray? = null
            try {
                bytes = android.util.Base64.decode(args.data, android.util.Base64.DEFAULT)
                transformed = SystemSecrets.transform(args.id, bytes, decrypt)
                val result = JSObject()
                result.put("data", android.util.Base64.encodeToString(transformed, android.util.Base64.NO_WRAP))
                invoke.resolve(result)
            } catch (_: Exception) {
                invoke.reject("系统安全存储不可用，或保存的数据无法解密")
            } finally {
                bytes?.fill(0)
                transformed?.fill(0)
            }
        }
    }
    override fun load(webView: WebView) {
        webView.settings.setSupportZoom(false)
        (activity as? AppCompatActivity)?.let { host ->
            host.onBackPressedDispatcher.addCallback(host, object : OnBackPressedCallback(true) {
                override fun handleOnBackPressed() {
                    webView.evaluateJavascript(
                        "window.dispatchEvent(new Event('cmcloud-back', {cancelable:true}))"
                    ) { result ->
                        // A cancelled event means the frontend handled navigation.
                        // At the root, background the task so transfers continue.
                        if (result != "false") host.moveTaskToBack(true)
                    }
                }
            })
        }
    }

    private val resolver = activity.applicationContext.contentResolver
    private val pickerBusy = AtomicBoolean(false)
    @Volatile private var activityForeground = false
    private val worker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "cmcloud-document-metadata").apply { isDaemon = true }
    }
    // Accessed only by worker; provider cursors are never held on the UI thread.
    private val scans = mutableMapOf<String, Scan>()
    private val pageSize = 200
    // Older supported Android releases permit 128 persisted grants per app.
    // Keep existing tasks safe; folder trees cover thousands of descendants.
    private val grantBudget = 128
    private val exportWorker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "cmcloud-document-export").apply { isDaemon = true }
    }
    private val exportBusy = AtomicBoolean(false)

    private val sharedWorker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "cmcloud-public-download").apply { isDaemon = true }
    }

    private fun publicRoot() =
        File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "CMCloudPlusPlus")

    private fun legacyAccess() = Build.VERSION.SDK_INT >= 29 ||
        activity.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

    @Command
    fun ensureSharedDownloads(invoke: Invoke) {
        activity.runOnUiThread {
            if (legacyAccess()) invoke.resolve(JSObject())
            else requestPermissionForAlias("legacyDownloads", invoke, "sharedDownloadsPermission")
        }
    }

    @PermissionCallback
    fun sharedDownloadsPermission(invoke: Invoke) {
        if (legacyAccess()) invoke.resolve(JSObject())
        else invoke.reject("请允许存储权限，以保存到公共下载目录。")
    }

    private fun sharedMediaItem(uri: Uri): Pair<Long, Boolean>? {
        require(uri.scheme == "content" && uri.authority == "media" &&
            uri.pathSegments.size == 3 && uri.pathSegments[1] == "downloads" &&
            uri.lastPathSegment?.toLongOrNull() != null) { "公共下载记录无效。" }
        val columns = arrayOf(MediaStore.MediaColumns.SIZE, MediaStore.MediaColumns.IS_PENDING,
            MediaStore.MediaColumns.RELATIVE_PATH)
        return resolver.query(uri, columns, null, null, null)?.use {
            if (!it.moveToFirst()) return@use null
            require(it.getString(2).startsWith(Environment.DIRECTORY_DOWNLOADS + "/CMCloudPlusPlus/")) {
                "公共下载路径无效。"
            }
            it.getLong(0) to (it.getInt(1) != 0)
        }
    }

    private fun legacyFile(uri: Uri): File {
        require(uri.scheme == "file") { "公共下载记录无效。" }
        val file = File(uri.path ?: error("公共下载路径无效。")).canonicalFile
        require(file.path.startsWith(publicRoot().canonicalPath + File.separator)) { "公共下载路径无效。" }
        return file
    }

    @Command
    fun prepareSharedDownload(invoke: Invoke) {
        val args = invoke.parseArgs(SharedDownloadArgs::class.java)
        sharedWorker.execute {
            try {
                val source = finishedDownload(args.path)
                require(legacyAccess()) { "请允许存储权限后重新开始任务。" }
                val folder = args.folder.split('/').filter { it.isNotEmpty() }
                require(folder.all { it != "." && it != ".." && '\\' !in it }) { "公共下载目录无效。" }
                val existing = args.uri?.let(Uri::parse)
                val uri = if (Build.VERSION.SDK_INT >= 29) {
                    if (existing != null && sharedMediaItem(existing) != null) existing
                    else {
                        val values = ContentValues().apply {
                            put(MediaStore.MediaColumns.DISPLAY_NAME, source.name)
                            put(MediaStore.MediaColumns.MIME_TYPE, mime(source))
                            put(MediaStore.MediaColumns.RELATIVE_PATH,
                                (listOf(Environment.DIRECTORY_DOWNLOADS, "CMCloudPlusPlus") + folder).joinToString("/") + "/")
                            put(MediaStore.MediaColumns.IS_PENDING, 1)
                        }
                        resolver.insert(MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values)
                            ?: error("无法创建公共下载文件。")
                    }
                } else if (existing != null && legacyFile(existing).isFile) existing
                else {
                    val dir = folder.fold(publicRoot()) { parent, name -> File(parent, name) }.canonicalFile
                    val root = publicRoot().canonicalPath
                    require(dir.path == root || dir.path.startsWith(root + File.separator)) { "公共下载目录无效。" }
                    require(dir.mkdirs() || dir.isDirectory) { "无法创建公共下载目录。" }
                    var target = File(dir, source.name)
                    var suffix = 0
                    while (!target.createNewFile()) {
                        suffix++
                        val extension = source.extension.takeIf { it.isNotEmpty() }?.let { ".$it" }.orEmpty()
                        target = File(dir, "${source.nameWithoutExtension} ($suffix)$extension")
                    }
                    Uri.fromFile(target)
                }
                invoke.resolve(JSObject().put("uri", uri.toString()))
            } catch (error: Exception) {
                invoke.reject(error.message ?: "无法创建公共下载文件。")
            }
        }
    }

    @Command
    fun finishSharedDownload(invoke: Invoke) {
        val args = invoke.parseArgs(SharedDownloadArgs::class.java)
        sharedWorker.execute {
            try {
                val source = finishedDownload(args.path)
                val uri = Uri.parse(args.uri ?: error("公共下载记录无效。"))
                val size = source.length()
                fun copy(output: FileOutputStream) {
                    val written = source.inputStream().use { it.copyTo(output, 256 * 1024) }
                    output.fd.sync()
                    require(written == size && source.length() == size) { "下载暂存文件在保存期间发生变化。" }
                }
                if (Build.VERSION.SDK_INT >= 29) {
                    val (storedSize, pending) = sharedMediaItem(uri) ?: error("公共下载文件已被删除。")
                    if (pending) {
                        val descriptor = resolver.openFileDescriptor(uri, "wt") ?: error("无法写入公共下载目录。")
                        android.os.ParcelFileDescriptor.AutoCloseOutputStream(descriptor).use(::copy)
                        val values = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
                        require(resolver.update(uri, values, null, null) == 1) { "无法发布公共下载文件。" }
                    } else require(storedSize == size) { "公共下载文件已被修改。" }
                } else {
                    val target = legacyFile(uri)
                    require(target.isFile) { "公共下载文件已被删除。" }
                    if (target.length() != size) {
                        require(target.length() == 0L) { "公共下载文件已被修改。" }
                        val part = File.createTempFile(".cmcloud-", ".part", target.parentFile)
                        try {
                            part.outputStream().use(::copy)
                            require(target.length() == 0L && part.renameTo(target)) { "无法发布公共下载文件。" }
                        } finally { if (part.exists()) part.delete() }
                    }
                }
                invoke.resolve(JSObject())
            } catch (error: Exception) {
                invoke.reject(error.message ?: "无法保存到公共下载目录。")
            }
        }
    }

    @Command
    fun discardSharedDownload(invoke: Invoke) {
        val uri = Uri.parse(invoke.parseArgs(SharedDownloadArgs::class.java).uri ?: "")
        sharedWorker.execute {
            try {
                if (Build.VERSION.SDK_INT >= 29) {
                    if (sharedMediaItem(uri)?.second == true) resolver.delete(uri, null, null)
                } else {
                    val file = legacyFile(uri)
                    if (file.isFile && file.length() == 0L) require(file.delete()) { "无法清理暂存下载。" }
                }
                invoke.resolve(JSObject())
            } catch (error: Exception) {
                invoke.reject(error.message ?: "无法清理暂存下载。")
            }
        }
    }

    private fun finishedDownload(path: String): File {
        val root = File(activity.applicationInfo.dataDir, "Downloads").canonicalFile
        val file = File(path).canonicalFile
        require(file.isFile && file.path.startsWith(root.path + File.separator)) { "只能打开应用下载目录中的已完成文件。" }
        return file
    }
    private fun mime(file: File): String = MimeTypeMap.getSingleton()
        .getMimeTypeFromExtension(file.extension.lowercase()) ?: "application/octet-stream"

    @Command
    fun openDownload(invoke: Invoke) {
        try {
            val path = invoke.parseArgs(DownloadArgs::class.java).path
            val uri: Uri
            val type: String
            if (path.startsWith("content://") && Build.VERSION.SDK_INT >= 29) {
                uri = Uri.parse(path)
                require(sharedMediaItem(uri)?.second == false) { "公共下载文件已被移动或删除。" }
                type = resolver.getType(uri) ?: "application/octet-stream"
            } else {
                val file = if (path.startsWith("file://")) legacyFile(Uri.parse(path)) else finishedDownload(path)
                require(file.isFile) { "公共下载文件已被移动或删除。" }
                uri = FileProvider.getUriForFile(activity, activity.packageName + ".downloads", file)
                type = mime(file)
            }
            activity.runOnUiThread {
                try {
                    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, type)
                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    activity.startActivity(intent)
                    invoke.resolve(JSObject())
                } catch (_: Exception) { invoke.reject("没有可打开此文件的应用。", "NO_FILE_VIEWER") }
            }
        } catch (error: Exception) { invoke.reject(error.message ?: "无法打开下载文件。", "DOWNLOAD_OPEN_FAILED") }
    }

    @Command
    fun exportDownload(invoke: Invoke) {
        if (!exportBusy.compareAndSet(false, true)) {
            invoke.reject("已有文件正在导出，请稍后重试。", "EXPORT_BUSY")
            return
        }
        try {
            val file = finishedDownload(invoke.parseArgs(DownloadArgs::class.java).path)
            val launched = launchPicker(invoke, Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = mime(file)
                putExtra(Intent.EXTRA_TITLE, file.name)
                addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            }, "downloadExported")
            if (!launched) exportBusy.set(false)
        } catch (error: Exception) {
            exportBusy.set(false)
            invoke.reject(error.message ?: "无法选择保存位置。", "DOWNLOAD_EXPORT_FAILED")
        }
    }

    @ActivityCallback
    fun downloadExported(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_CANCELED) {
            pickerBusy.set(false); exportBusy.set(false)
            invoke.resolve(JSObject().put("saved", false))
            return
        }
        exportWorker.execute {
            var destination: Uri? = null
            try {
                require(result.resultCode == Activity.RESULT_OK) { "保存位置选择失败。" }
                destination = result.data?.data ?: error("系统未返回保存位置。")
                val file = finishedDownload(invoke.parseArgs(DownloadArgs::class.java).path)
                val initialSize = file.length()
                var written = 0L
                resolver.openOutputStream(destination, "wt")?.use { output ->
                    file.inputStream().use { input ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) {
                            if (Thread.currentThread().isInterrupted) error("文件导出已停止。")
                            val count = input.read(buffer)
                            if (count < 0) break
                            output.write(buffer, 0, count); written += count
                        }
                        output.flush()
                    }
                } ?: error("文件来源不支持写入。")
                require(written == initialSize && file.length() == initialSize) { "下载文件在导出期间发生变化。" }
                invoke.resolve(JSObject().put("saved", true))
            } catch (error: Exception) {
                // ACTION_CREATE_DOCUMENT created a new destination; remove a failed partial copy.
                destination?.let { try { DocumentsContract.deleteDocument(resolver, it) } catch (_: Exception) {} }
                invoke.reject(error.message ?: "文件导出失败，原始下载仍保留。", "DOWNLOAD_EXPORT_FAILED")
            } finally { pickerBusy.set(false); exportBusy.set(false) }
        }
    }

    @Command
    fun beginTransfers(invoke: Invoke) {
        activity.runOnUiThread {
            val stats = invoke.parseArgs(TransferStats::class.java)
            if (TransferSession.userStopped && !activityForeground) {
                invoke.resolve(foregroundState())
                return@runOnUiThread
            }
            TransferSession.userStopped = false
            TransferSession.renew(stats)
            activity.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            requestProtection(false)
            if (TransferForegroundService.active || (Build.VERSION.SDK_INT >= 34 && TransferJobService.active)) {
                invoke.resolve(foregroundState())
                return@runOnUiThread
            }
            if (Build.VERSION.SDK_INT >= 34) {
                val scheduler = activity.getSystemService(JobScheduler::class.java)
                if (scheduler.getPendingJob(TransferSession.JOB) != null) {
                    TransferSession.reason = "正在等待可用网络与系统调度。"
                    invoke.resolve(foregroundState())
                    return@runOnUiThread
                }
                if (activityForeground) {
                    try {
                        val job = JobInfo.Builder(TransferSession.JOB, ComponentName(activity, TransferJobService::class.java))
                            .setUserInitiated(true)
                            .setRequiredNetwork(NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET).build())
                            .setEstimatedNetworkBytes(stats.downloadRemaining.coerceAtLeast(0L), stats.uploadRemaining.coerceAtLeast(0L))
                            .build()
                        if (scheduler.schedule(job) == JobScheduler.RESULT_SUCCESS) {
                            TransferSession.reason = null
                            awaitBackgroundStart(invoke)
                            return@runOnUiThread
                        }
                    } catch (_: Exception) { /* Use the dataSync fallback below. */ }
                }
            }
            val exempt = activity.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(activity.packageName)
            if (!activityForeground && !exempt && Build.VERSION.SDK_INT >= 31) {
                TransferSession.reason = "请回到应用后启动后台传输，并允许后台高耗电。"
                invoke.resolve(foregroundState())
                return@runOnUiThread
            }
            try {
                TransferSession.reason = null
                val intent = Intent(activity, TransferForegroundService::class.java)
                if (Build.VERSION.SDK_INT >= 26) activity.applicationContext.startForegroundService(intent)
                else activity.applicationContext.startService(intent)
                awaitBackgroundStart(invoke)
            } catch (_: Exception) {
                TransferSession.reason = "Android 当前限制后台传输启动，请回到应用后继续。"
                invoke.resolve(foregroundState())
            }
        }
    }

    private fun awaitBackgroundStart(invoke: Invoke) {
        val handler = Handler(Looper.getMainLooper())
        val started = System.nanoTime()
        handler.post(object : Runnable {
            override fun run() {
                if (TransferForegroundService.active || (Build.VERSION.SDK_INT >= 34 && TransferJobService.active) ||
                    TransferSession.reason != null || System.nanoTime() - started >= 3_000_000_000L) {
                    invoke.resolve(foregroundState())
                } else handler.postDelayed(this, 100)
            }
        })
    }

    @Command
    fun requestTransferProtection(invoke: Invoke) {
        activity.runOnUiThread {
            requestProtection(true)
            invoke.resolve(foregroundState())
        }
    }

    private fun requestProtection(force: Boolean) {
        if (!activityForeground) return
        val preferences = activity.getSharedPreferences("transfer-protection", Context.MODE_PRIVATE)
        if (Build.VERSION.SDK_INT >= 33 && activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED &&
            (force || !preferences.getBoolean("notifications-asked", false))) {
            preferences.edit().putBoolean("notifications-asked", true).apply()
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 13903)
            return
        }
        val power = activity.getSystemService(PowerManager::class.java)
        if (!power.isIgnoringBatteryOptimizations(activity.packageName) && (force || !preferences.getBoolean("battery-asked", false))) {
            preferences.edit().putBoolean("battery-asked", true).apply()
            try {
                activity.startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                    Uri.parse("package:${activity.packageName}")))
            } catch (_: Exception) {
                activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                    Uri.parse("package:${activity.packageName}")))
            }
        } else if (force) {
            activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${activity.packageName}")))
        }
    }

    @Command
    fun endTransfers(invoke: Invoke) {
        activity.runOnUiThread {
            TransferSession.end()
            activity.window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            if (Build.VERSION.SDK_INT >= 34) activity.getSystemService(JobScheduler::class.java).cancel(TransferSession.JOB)
            activity.applicationContext.stopService(Intent(activity, TransferForegroundService::class.java))
            TransferSession.reason = null
            invoke.resolve(JSObject())
        }
    }

    @Command
    fun foregroundStatus(invoke: Invoke) { invoke.resolve(foregroundState()) }

    private fun foregroundState(): JSObject {
        val notifications = Build.VERSION.SDK_INT < 33 ||
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        val active = TransferForegroundService.active || (Build.VERSION.SDK_INT >= 34 && TransferJobService.active)
        return JSObject().put("supported", true).put("active", active)
            .put("notificationPermission", notifications).put("userStopped", TransferSession.userStopped).put("reason", TransferSession.reason)
    }

    @Command
    fun pickFiles(invoke: Invoke) {
        launchPicker(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }, "filesPicked")
    }

    @Command
    fun pickDirectory(invoke: Invoke) {
        launchPicker(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION or Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
        }, "directoryPicked")
    }

    private fun launchPicker(invoke: Invoke, intent: Intent, callback: String): Boolean {
        if (!pickerBusy.compareAndSet(false, true)) {
            invoke.reject("文件选择器已打开，请完成当前选择。", "PICKER_BUSY")
            return false
        }
        activity.runOnUiThread {
            try { startActivityForResult(invoke, intent, callback) }
            catch (_: Exception) {
                pickerBusy.set(false)
                if (callback == "downloadExported") exportBusy.set(false)
                invoke.reject("无法打开系统文件选择器。", "PICKER_UNAVAILABLE")
            }
        }
        return true
    }

    @ActivityCallback
    fun filesPicked(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_CANCELED) {
            pickerBusy.set(false)
            invoke.resolve(JSObject().put("files", JSArray()))
            return
        }
        worker.execute {
            val newlyGranted = mutableListOf<Uri>()
            try {
                if (result.resultCode != Activity.RESULT_OK) error("文件选择失败，请重试。")
                val data = result.data ?: error("系统未返回所选文件。")
                val uris = linkedSetOf<Uri>()
                data.data?.let { uris.add(it) }
                data.clipData?.let { clip ->
                    if (clip.itemCount > 64) error("单次选择最多 64 个文件；批量上传请使用上传文件夹。")
                    for (index in 0 until clip.itemCount) uris.add(clip.getItemAt(index).uri)
                }
                if (uris.isEmpty()) error("系统未返回所选文件。")
                if (uris.size > 64) error("单次选择最多 64 个文件；批量上传请使用上传文件夹。")
                val existing = persistedReads()
                if (existing.size + uris.count { it.toString() !in existing } > grantBudget) {
                    error("已接近系统文件授权数量上限；批量上传请选择文件夹，保留现有任务的读取权限。")
                }
                val files = JSArray()
                for (uri in uris) {
                    persistRead(uri, data.flags, existing, newlyGranted)
                    files.put(fileMetadata(uri))
                }
                // Never let an unexpectedly low vendor limit invalidate old jobs.
                if (!persistedReads().containsAll(existing + uris.map { it.toString() })) {
                    error("系统未保留文件读取授权，请改用上传文件夹。")
                }
                invoke.resolve(JSObject().put("files", files))
            } catch (error: Exception) {
                rollbackGrants(newlyGranted)
                invoke.reject(error.message ?: "无法读取所选文件。", "DOCUMENT_ACCESS_FAILED")
            } finally { pickerBusy.set(false) }
        }
    }

    @ActivityCallback
    fun directoryPicked(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_CANCELED) {
            pickerBusy.set(false)
            invoke.resolve(JSObject().put("directory", null))
            return
        }
        worker.execute {
            val newlyGranted = mutableListOf<Uri>()
            try {
                if (result.resultCode != Activity.RESULT_OK) error("目录选择失败，请重试。")
                val data = result.data ?: error("系统未返回所选目录。")
                val tree = data.data ?: error("系统未返回所选目录。")
                if (!DocumentsContract.isTreeUri(tree)) error("系统返回的目录格式无效。")
                val existing = persistedReads()
                if (tree.toString() !in existing && existing.size >= grantBudget) {
                    error("系统读取授权数量已满，请先完成已有传输任务。")
                }
                persistRead(tree, data.flags, existing, newlyGranted)
                val document = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
                val metadata = fileMetadata(document)
                val directory = JSObject().put("path", tree.toString()).put("name", metadata.getString("name"))
                invoke.resolve(JSObject().put("directory", directory))
            } catch (error: Exception) {
                rollbackGrants(newlyGranted)
                invoke.reject(error.message ?: "无法读取所选目录。", "DIRECTORY_ACCESS_FAILED")
            } finally { pickerBusy.set(false) }
        }
    }

    private fun persistedReads(): Set<String> = resolver.persistedUriPermissions
        .filter { it.isReadPermission }.map { it.uri.toString() }.toSet()

    private fun persistRead(uri: Uri, flags: Int, existing: Set<String>, added: MutableList<Uri>) {
        if (uri.scheme != "content") error("系统返回的文档 URI 不受支持。")
        if (uri.toString() in existing) return
        if (flags and Intent.FLAG_GRANT_READ_URI_PERMISSION == 0 ||
            flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION == 0) {
            error("此文件来源不提供持久读取权限，请选择本机文件或文件夹。")
        }
        try { resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        catch (_: SecurityException) { error("文件来源未授予持久读取权限，请重新选择。") }
        added.add(uri)
    }

    private fun rollbackGrants(uris: List<Uri>) {
        for (uri in uris) {
            try { resolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
            catch (_: Exception) { /* Existing permissions are never released. */ }
        }
    }

    private fun fileMetadata(uri: Uri): JSObject {
        var name: String? = null
        var size: Long? = null
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (!cursor.moveToFirst()) error("所选文件已删除或无法读取。")
            val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            val sizeIndex = cursor.getColumnIndex(OpenableColumns.SIZE)
            if (nameIndex >= 0 && !cursor.isNull(nameIndex)) name = cursor.getString(nameIndex)
            if (sizeIndex >= 0 && !cursor.isNull(sizeIndex)) size = cursor.getLong(sizeIndex).takeIf { it >= 0 }
        }
        if (size == null) {
            // Descriptor metadata only: never read, duplicate, or copy file bytes.
            try { resolver.openAssetFileDescriptor(uri, "r")?.use { descriptor ->
                size = descriptor.length.takeIf { it >= 0 }
            } } catch (_: Exception) { /* Some providers cannot report a size. */ }
        }
        val fallback = uri.lastPathSegment?.substringAfterLast('/') ?: "文档"
        return JSObject().put("path", uri.toString()).put("name", name?.takeIf { it.isNotBlank() } ?: fallback)
            .put("size", size)
    }

    @Command
    fun listChildren(invoke: Invoke) {
        worker.execute {
            var scanId: String? = null
            try {
                closeExpiredScans()
                val args = invoke.parseArgs(ChildrenArgs::class.java)
                val tree = Uri.parse(args.tree)
                if (tree.scheme != "content" || !DocumentsContract.isTreeUri(tree)) error("目录 URI 无效。")
                val treeId = DocumentsContract.getTreeDocumentId(tree)
                val parent = args.parent?.let(Uri::parse)
                    ?: DocumentsContract.buildDocumentUriUsingTree(tree, treeId)
                if (parent.authority != tree.authority || !DocumentsContract.isTreeUri(parent) ||
                    DocumentsContract.getTreeDocumentId(parent) != treeId) error("子目录超出已授权范围。")
                val scan: Scan
                if (args.cursor != null) {
                    scanId = args.cursor
                    scan = scans[scanId] ?: error("目录读取会话已过期，请重试。")
                    if (scan.tree != tree || scan.parent != parent) error("目录读取会话不匹配。")
                } else {
                    if (scans.size >= 32) error("同时打开的目录过多，请稍后重试。")
                    val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getDocumentId(parent))
                    val projection = arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                        DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_SIZE,
                        DocumentsContract.Document.COLUMN_MIME_TYPE)
                    val cursor = resolver.query(children, projection, null, null, null)
                        ?: error("文件来源无法读取目录内容。")
                    scanId = UUID.randomUUID().toString()
                    scan = Scan(tree, parent, cursor)
                    scans[scanId] = scan
                }
                val entries = JSArray()
                val cursor = scan.cursor
                val idIndex = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
                val nameIndex = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
                val sizeIndex = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_SIZE)
                val mimeIndex = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_MIME_TYPE)
                var exhausted = false
                while (entries.length() < pageSize) {
                    if (!cursor.moveToNext()) { exhausted = true; break }
                    val id = cursor.getString(idIndex) ?: continue
                    val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                    val name = cursor.getString(nameIndex)?.takeIf { it.isNotBlank() } ?: id.substringAfterLast('/')
                    val size = if (sizeIndex >= 0 && !cursor.isNull(sizeIndex)) cursor.getLong(sizeIndex).takeIf { it >= 0 } else null
                    entries.put(JSObject().put("path", uri.toString()).put("name", name).put("size", size)
                        .put("isDirectory", cursor.getString(mimeIndex) == DocumentsContract.Document.MIME_TYPE_DIR))
                }
                scan.touched = System.nanoTime()
                if (exhausted) { scans.remove(scanId)?.cursor?.close() }
                invoke.resolve(JSObject().put("entries", entries).put("nextCursor", if (exhausted) null else scanId))
            } catch (error: Exception) {
                scanId?.let { scans.remove(it)?.cursor?.close() }
                invoke.reject(error.message ?: "目录读取失败，请重新选择目录。", "DIRECTORY_SCAN_FAILED")
            }
        }
    }

    @Command
    fun closeScan(invoke: Invoke) {
        worker.execute {
            try {
                val args = invoke.parseArgs(CloseScanArgs::class.java)
                scans.remove(args.cursor)?.cursor?.close()
                invoke.resolve(JSObject())
            } catch (_: Exception) { invoke.reject("无法结束目录读取会话。", "DIRECTORY_SCAN_FAILED") }
        }
    }

    private fun closeExpiredScans() {
        val expired = scans.filterValues { System.nanoTime() - it.touched > 120_000_000_000L }.keys.toList()
        for (id in expired) scans.remove(id)?.cursor?.close()
    }

    // No-argument lifecycle hooks remain available in Tauri 2.11 and 2.12.
    @Suppress("OVERRIDE_DEPRECATION")
    override fun onResume() { activityForeground = true }
    @Suppress("OVERRIDE_DEPRECATION")
    override fun onPause() { activityForeground = false }

    override fun onDestroy(activity: AppCompatActivity) {
        activityForeground = false
        pickerBusy.set(false)
        worker.execute { scans.values.forEach { it.cursor.close() }; scans.clear() }
        worker.shutdown()
        exportWorker.shutdownNow()
    }
}
