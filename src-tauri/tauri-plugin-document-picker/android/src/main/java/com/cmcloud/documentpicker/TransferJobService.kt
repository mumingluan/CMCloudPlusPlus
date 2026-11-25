package com.cmcloud.documentpicker

import android.app.job.JobParameters
import android.app.job.JobService
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.RequiresApi

/** Android 14+ user-initiated long transfer job. Rust owns the actual I/O. */
@RequiresApi(34)
class TransferJobService : JobService() {
    private val main = Handler(Looper.getMainLooper())
    private var params: JobParameters? = null
    private var protection: TransferProtection? = null
    private val heartbeat = object : Runnable {
        override fun run() {
            val job = params ?: return
            if (!TransferSession.valid()) {
                if (TransferSession.requested) TransferSession.reason = "传输运行时已停止，请打开应用恢复队列。"
                release()
                jobFinished(job, false)
                return
            }
            protection?.refresh()
            setNotification(job, TransferSession.NOTIFICATION, TransferSession.notification(this@TransferJobService), JOB_END_NOTIFICATION_POLICY_REMOVE)
            main.postDelayed(this, 5_000)
        }
    }
    override fun onStartJob(params: JobParameters): Boolean {
        if (Build.VERSION.SDK_INT < 34 || !TransferSession.valid()) return false
        this.params = params
        setNotification(params, TransferSession.NOTIFICATION, TransferSession.notification(this), JOB_END_NOTIFICATION_POLICY_REMOVE)
        protection = TransferProtection(applicationContext).apply { acquire() }
        active = true
        TransferSession.reason = null
        main.post(heartbeat)
        return true
    }
    override fun onStopJob(params: JobParameters): Boolean {
        val userStopped = params.stopReason == JobParameters.STOP_REASON_USER
        if (userStopped) {
            TransferSession.userStopped = true
            TransferSession.end()
            TransferSession.reason = "后台传输已由系统或用户停止，请打开应用继续。"
        }
        release()
        return !userStopped && TransferSession.valid()
    }
    private fun release() {
        active = false
        main.removeCallbacks(heartbeat)
        protection?.release(); protection = null
        params = null
    }
    override fun onDestroy() { release(); super.onDestroy() }
    companion object {
        @Volatile var active = false
            private set
    }
}
