package com.cmcloud.documentpicker

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import java.lang.ref.WeakReference

/** Keeps the current Rust transfer process eligible to run in the background.
 * This service does not own transfer data or restart a killed Rust runtime. */
class TransferForegroundService : Service() {
    private val main = Handler(Looper.getMainLooper())
    private var leaseDeadline = 0L
    private var wakeLock: PowerManager.WakeLock? = null
    private val leaseMillis = 120_000L
    private val watchLease = object : Runnable {
        override fun run() {
            if (System.nanoTime() >= leaseDeadline) {
                stopSafely("后台传输保活已结束，请回到应用检查任务。")
            } else main.postDelayed(this, 15_000)
        }
    }

    override fun onCreate() {
        super.onCreate()
        current = WeakReference(this)
        try {
            val manager = getSystemService(NotificationManager::class.java)
            if (Build.VERSION.SDK_INT >= 26) {
                manager.createNotificationChannel(NotificationChannel(
                    CHANNEL, "云盘文件传输", NotificationManager.IMPORTANCE_LOW,
                ).apply { description = "上传和下载进行时保持后台传输，并显示返回应用的入口。" })
            }
            val launch = packageManager.getLaunchIntentForPackage(packageName)
            val notification = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL)
                else Notification.Builder(this)
            notification.setSmallIcon(android.R.drawable.stat_sys_upload)
                .setContentTitle("云盘传输正在进行")
                .setContentText("传输任务已保存，点按返回应用查看进度")
                .setOngoing(true).setOnlyAlertOnce(true).setCategory(Notification.CATEGORY_PROGRESS)
            if (launch != null) notification.setContentIntent(PendingIntent.getActivity(
                this, 0, launch, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            ))
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(NOTIFICATION, notification.build(), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            } else startForeground(NOTIFICATION, notification.build())
            wakeLock = getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:transfers")
                .apply { setReferenceCounted(false) }
            active = true
            reason = null
            renewLease()
            main.post(watchLease)
        } catch (_: Exception) {
            stopSafely("系统未允许后台传输保活，请保持应用在前台。")
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (active) renewLease()
        // Restarting a Java service alone cannot recreate the Rust transfer
        // runtime. Recovery comes from the persistent queue on the next launch.
        return START_NOT_STICKY
    }

    private fun renewLease() {
        if (!active) return
        leaseDeadline = System.nanoTime() + leaseMillis * 1_000_000
        wakeLock?.let {
            if (it.isHeld) it.release()
            it.acquire(leaseMillis)
        }
    }

    private fun stopSafely(message: String?) {
        active = false
        reason = message
        main.removeCallbacks(watchLease)
        wakeLock?.let { if (it.isHeld) it.release() }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    // Android 15+ dataSync FGS has a shared six-hour background budget in 24h.
    // Stop promptly on timeout to avoid an ANR; the durable queue survives.
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSafely("Android 已达到后台传输时限，请回到应用继续。")
    }

    override fun onDestroy() {
        active = false
        main.removeCallbacks(watchLease)
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        current = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        private const val CHANNEL = "cmcloud-transfers"
        private const val NOTIFICATION = 13901
        @Volatile var active = false
            private set
        @Volatile var reason: String? = null
        @Volatile private var current: WeakReference<TransferForegroundService>? = null

        fun refreshLease(): Boolean {
            val service = current?.get() ?: return false
            if (!active) return false
            service.main.post { service.renewLease() }
            return true
        }
    }
}
