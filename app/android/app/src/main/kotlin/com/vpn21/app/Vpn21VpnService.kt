package com.vpn21.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import androidx.core.app.NotificationCompat

/**
 * VpnService that creates the TUN interface and hands the fd back to the
 * Dart side through [waitForTun].  The Rust core then adopts the fd via
 * `vpn21_start`.  Per-app rules (allow/deny lists) are pulled from
 * SharedPreferences so the user can configure them from the Flutter UI
 * without restarting the service.
 */
class Vpn21VpnService : VpnService() {

    private var pfd: ParcelFileDescriptor? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_START) {
            startForegroundChannel()
            buildTun()
        }
        return START_NOT_STICKY
    }

    private fun buildTun() {
        val builder = Builder()
            .setSession("vpn21")
            .setMtu(1500)
            .addAddress(TUN_IP, TUN_PREFIX)
            .addRoute("0.0.0.0", 0)
            .addRoute("::", 0)
            .addDnsServer(TUN_IP)
            .setBlocking(true)

        // Per-app VPN rules — reads the `SharedPreferences` keys stored by
        // the Flutter AppsPage.
        val sp = getSharedPreferences("FlutterSharedPreferences", Context.MODE_PRIVATE)
        val mode = sp.getString("flutter.apps_mode", "disabled") ?: "disabled"
        val selRaw = sp.getString("flutter.apps_sel", null)
        val sel = selRaw
            ?.removePrefix("[")
            ?.removeSuffix("]")
            ?.split(",")
            ?.map { it.trim().removeSurrounding("\"") }
            ?.filter { it.isNotEmpty() }
            ?: emptyList()
        when (mode) {
            "included" -> sel.forEach { runCatching { builder.addAllowedApplication(it) } }
            "excluded" -> sel.forEach { runCatching { builder.addDisallowedApplication(it) } }
            else -> {}
        }

        val fd = builder.establish()
        pfd = fd
        if (fd == null) {
            pushResult(null, "VpnService.establish() returned null")
            stopSelf()
            return
        }
        val tun = mapOf(
            "fd" to fd.fd,
            "mtu" to 1500,
            "ipv4" to TUN_IP,
            "mask" to TUN_PREFIX,
            "dnsPort" to 53,
        )
        pushResult(tun, null)
    }

    private fun startForegroundChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(NotificationManager::class.java)
            nm?.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "vpn21", NotificationManager.IMPORTANCE_LOW)
            )
        }
        val pi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n: Notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_vpn_ic)
            .setContentTitle("vpn21 is active")
            .setContentText("Traffic is routed through Tor.")
            .setOngoing(true)
            .setContentIntent(pi)
            .build()
        startForeground(NOTIF_ID, n)
    }

    override fun onDestroy() {
        super.onDestroy()
        runCatching { pfd?.close() }
        pfd = null
    }

    companion object {
        const val ACTION_START = "com.vpn21.app.START"
        const val EXTRA_PROFILE = "profile"
        private const val CHANNEL_ID = "vpn21"
        private const val NOTIF_ID = 1

        private const val TUN_IP = "10.19.21.1"
        private const val TUN_PREFIX = 24

        // Very small in-process bus so that MainActivity can await the fd.
        private var pending: ((Map<String, Any?>?, String?) -> Unit)? = null
        private var latestTun: Map<String, Any?>? = null
        private var latestErr: String? = null

        fun waitForTun(cb: (Map<String, Any?>?, String?) -> Unit) {
            val lt = latestTun
            val le = latestErr
            if (lt != null || le != null) {
                cb(lt, le)
                latestTun = null
                latestErr = null
                return
            }
            pending = cb
        }

        fun pushResult(tun: Map<String, Any?>?, err: String?) {
            val p = pending
            if (p != null) {
                p(tun, err)
                pending = null
            } else {
                latestTun = tun
                latestErr = err
            }
        }
    }
}
