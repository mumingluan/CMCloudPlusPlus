package com.cmcloud.documentpicker

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiManager
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import app.tauri.annotation.InvokeArg
import java.util.Locale

@InvokeArg
class TransferStats {
    var totalJobs: Long = 0
    var completedJobs: Long = 0
    var activeJobs: Long = 0
    var queuedJobs: Long = 0
    var totalBytes: Long = 0
    var completedBytes: Long = 0
    var uploadSpeed: Double = 0.0
    var downloadSpeed: Double = 0.0
    var uploadRemaining: Long = 0
    var downloadRemaining: Long = 0
}

/** Native heartbeat, independent of WebView timers and visibility. */
internal object TransferSession {
    const val CHANNEL = "cmcloud-transfers"
    const val NOTIFICATION = 13901
    const val JOB = 13902
    const val LEASE = 180_000L
    @Volatile var stats = TransferStats()
    @Volatile var requested = false
    @Volatile var deadline = 0L
    @Volatile var reason: String? = null
    @Volatile var userStopped = false
    fun renew(value: TransferStats) {
        stats = value
        deadline = SystemClock.elapsedRealtime() + LEASE
        requested = true
    }
    fun valid() = requested && SystemClock.elapsedRealtime() < deadline
    fun end() { requested = false; deadline = 0L }
    private fun bytes(value: Double): String {
        val units = arrayOf("B", "KiB", "MiB", "GiB", "TiB", "PiB")
        var amount = value.coerceAtLeast(0.0)
        var unit = 0
        while (amount >= 1024.0 && unit < units.size - 1) { amount /= 1024.0; unit++ }
        return String.format(Locale.ROOT, "%.1f %s", amount, units[unit])
    }
    @Suppress("DEPRECATION")
    fun notification(context: Context): Notification {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel(
            CHANNEL, "云盘文件传输", NotificationManager.IMPORTANCE_LOW,
        ).apply { description = "后台上传下载的速度和进度" })
        val value = stats
        val percent = if (value.totalBytes > 0) value.completedBytes.toDouble() / value.totalBytes * 100.0
            else if (value.totalJobs > 0) value.completedJobs.toDouble() / value.totalJobs * 100.0 else 0.0
        val details = "${bytes(value.completedBytes.toDouble())} / ${bytes(value.totalBytes.toDouble())} · ${value.completedJobs}/${value.totalJobs} 项"
        val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(context, CHANNEL) else Notification.Builder(context)
        builder.setSmallIcon(android.R.drawable.stat_sys_upload)
            .setContentTitle(String.format(Locale.ROOT, "CMCloud++ 正在传输 · %.1f%%", percent.coerceIn(0.0, 100.0)))
            .setContentText("↑ ${bytes(value.uploadSpeed)}/s · ↓ ${bytes(value.downloadSpeed)}/s")
            .setSubText(details)
            .setStyle(Notification.BigTextStyle().bigText("↑ ${bytes(value.uploadSpeed)}/s · ↓ ${bytes(value.downloadSpeed)}/s\n$details"))
            .setProgress(1000, (percent.coerceIn(0.0, 100.0) * 10).toInt(), value.totalJobs == 0L)
            .setOngoing(true).setOnlyAlertOnce(true).setCategory(Notification.CATEGORY_PROGRESS)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { launch ->
            builder.setContentIntent(PendingIntent.getActivity(context, 0, launch,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
        }
        return builder.build()
    }
}

/** Hold CPU and radio resources only for a live, non-idle Rust queue. */
internal class TransferProtection(private val context: Context) {
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    private var connectivity: ConnectivityManager? = null
    private var callback: ConnectivityManager.NetworkCallback? = null
    @Suppress("DEPRECATION")
    fun acquire() {
        wake = context.getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "${context.packageName}:transfers")
            .apply { setReferenceCounted(false) }
        refresh()
        try {
            wifi = (context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager)
                ?.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "${context.packageName}:transfers")
                ?.apply { setReferenceCounted(false); acquire() }
        } catch (_: Exception) { wifi = null }
        try {
            connectivity = context.getSystemService(ConnectivityManager::class.java)
            callback = object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) { if (TransferSession.valid()) TransferSession.reason = null }
            }
            connectivity?.requestNetwork(NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET).build(), callback!!)
        } catch (_: Exception) { callback = null }
    }
    fun refresh() { wake?.acquire(TransferSession.LEASE + 30_000L) }
    fun release() {
        callback?.let { try { connectivity?.unregisterNetworkCallback(it) } catch (_: Exception) {} }
        callback = null
        wifi?.let { if (it.isHeld) it.release() }; wifi = null
        wake?.let { if (it.isHeld) it.release() }; wake = null
    }
}
