package com.cmcloud.documentpicker

import android.app.Activity
import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import java.io.File
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
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

private class Scan(
    val tree: Uri,
    val parent: Uri,
    val cursor: Cursor,
    var touched: Long = System.nanoTime(),
)

/** SAF metadata and persisted read grants, without staging document bytes. */
@TauriPlugin
class DocumentPickerPlugin(private val activity: Activity) : Plugin(activity) {
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

    private fun finishedDownload(path: String): File {
        val root = File(activity.applicationInfo.dataDir, "io.cmcloud.plus/Downloads").canonicalFile
        val file = File(path).canonicalFile
        require(file.isFile && file.path.startsWith(root.path + File.separator)) { "只能打开应用下载目录中的已完成文件。" }
        return file
    }
    private fun mime(file: File): String = MimeTypeMap.getSingleton()
        .getMimeTypeFromExtension(file.extension.lowercase()) ?: "application/octet-stream"

    @Command
    fun openDownload(invoke: Invoke) {
        try {
            val file = finishedDownload(invoke.parseArgs(DownloadArgs::class.java).path)
            val uri = FileProvider.getUriForFile(activity, activity.packageName + ".downloads", file)
            activity.runOnUiThread {
                try {
                    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime(file))
                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    activity.startActivity(intent)
                    invoke.resolve(JSObject())
                } catch (_: Exception) { invoke.reject("没有可打开此文件的应用，请使用导出保存。", "NO_FILE_VIEWER") }
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
            if (TransferForegroundService.refreshLease()) {
                invoke.resolve(foregroundState())
                return@runOnUiThread
            }
            if (!activityForeground && Build.VERSION.SDK_INT >= 31) {
                if (TransferForegroundService.reason == null) {
                    TransferForegroundService.reason = "请回到应用后启动后台传输。"
                }
                invoke.resolve(foregroundState())
                return@runOnUiThread
            }
            try {
                TransferForegroundService.reason = null
                val intent = Intent(activity, TransferForegroundService::class.java)
                if (Build.VERSION.SDK_INT >= 26) activity.applicationContext.startForegroundService(intent)
                else activity.applicationContext.startService(intent)
                val handler = Handler(Looper.getMainLooper())
                val started = System.nanoTime()
                val awaitStart = object : Runnable {
                    override fun run() {
                        if (TransferForegroundService.active || TransferForegroundService.reason != null ||
                            System.nanoTime() - started >= 3_000_000_000L) {
                            invoke.resolve(foregroundState())
                        } else handler.postDelayed(this, 100)
                    }
                }
                handler.post(awaitStart)
            } catch (_: Exception) {
                TransferForegroundService.reason = "Android 当前限制后台传输启动，请回到应用后继续。"
                invoke.resolve(foregroundState())
            }
        }
    }

    @Command
    fun endTransfers(invoke: Invoke) {
        activity.runOnUiThread {
            activity.applicationContext.stopService(Intent(activity, TransferForegroundService::class.java))
            TransferForegroundService.reason = null
            invoke.resolve(JSObject())
        }
    }

    @Command
    fun foregroundStatus(invoke: Invoke) { invoke.resolve(foregroundState()) }

    private fun foregroundState(): JSObject {
        val notifications = Build.VERSION.SDK_INT < 33 ||
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        return JSObject().put("supported", true).put("active", TransferForegroundService.active)
            .put("notificationPermission", notifications).put("reason", TransferForegroundService.reason)
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
