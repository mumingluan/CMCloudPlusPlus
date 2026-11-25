package com.cmcloud.documentpicker

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.app.NotificationManager

/** Foreground dataSync fallback for Android 7–13 or a rejected UIDT job. */
class TransferForegroundService : Service() {
    private val main = Handler(Looper.getMainLooper())
    private var protection: TransferProtection? = null
    private val heartbeat = object : Runnable {
        override fun run() {
            if (!TransferSession.valid()) {
                stopSafely(if (TransferSession.requested) "传输运行时已停止，请打开应用恢复队列。" else null)
                return
            }
            protection?.refresh()
            getSystemService(NotificationManager::class.java).notify(TransferSession.NOTIFICATION, TransferSession.notification(this@TransferForegroundService))
            main.postDelayed(this, 5_000)
        }
    }
    override fun onCreate() {
        super.onCreate()
        try {
            val notification = TransferSession.notification(this)
            if (Build.VERSION.SDK_INT >= 29) startForeground(TransferSession.NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            else startForeground(TransferSession.NOTIFICATION, notification)
            protection = TransferProtection(applicationContext).apply { acquire() }
            active = true
            TransferSession.reason = null
            main.post(heartbeat)
        } catch (_: Exception) { stopSafely("系统未允许后台传输，请在应用中允许后台高耗电。") }
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // A restarted Java service cannot reconstruct a killed Rust runtime.
        return START_NOT_STICKY
    }
    private fun stopSafely(message: String?) {
        TransferSession.reason = message
        active = false
        main.removeCallbacks(heartbeat)
        protection?.release(); protection = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }
    override fun onTimeout(startId: Int, fgsType: Int) {
        TransferSession.userStopped = true
        stopSafely("Android 已达到前台服务时限，请打开应用继续传输。")
    }
    override fun onDestroy() {
        active = false
        main.removeCallbacks(heartbeat)
        protection?.release(); protection = null
        super.onDestroy()
    }
    override fun onBind(intent: Intent?): IBinder? = null
    companion object {
        @Volatile var active = false
            private set
    }
}
