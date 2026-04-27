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
import org.json.JSONObject

/**
 * Foreground VpnService that creates the TUN interface and hands the fd
 * **directly to Rust** via [Vpn21Native.nativeStartWithFd].  The fd never
 * leaves the native process — Dart only ever asks the service to start /
 * stop via a MethodChannel.
 *
 * The per-app filter (allow / deny lists) is read from the same
 * SharedPreferences keys the Flutter `AppsPage` writes to, so the user
 * does not need to restart the service after editing the list.
 */
class Vpn21VpnService : VpnService() {

    private var pfd: ParcelFileDescriptor? = null

    override fun onCreate() {
        super.onCreate()
        // Bind libvpn21.so + run one-shot core init using the app-private
        // files dir; safe to call repeatedly (idempotent in Rust).
        Vpn21Native.nativeInit(filesDir.absolutePath, /* verbose = */ false)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                startForegroundChannel()
                val profile = intent.getStringExtra(EXTRA_PROFILE) ?: ""
                buildAndStart(profile)
            }
            ACTION_STOP -> {
                shutdown()
            }
        }
        return START_NOT_STICKY
    }

    private fun buildAndStart(profileJson: String) {
        val builder = Builder()
            .setSession("vpn21")
            .setMtu(1500)
            .addAddress(TUN_IP, TUN_PREFIX)
            .addRoute("0.0.0.0", 0)
            .addRoute("::", 0)
            .addDnsServer(TUN_IP)
            .setBlocking(true)

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
            pushResult(false, "VpnService.establish() returned null")
            stopSelf()
            return
        }

        // Hand the fd straight to Rust — leaf will adopt it inside its own
        // inbound-tun and start the netstack on top.  The Kotlin side
        // keeps the ParcelFileDescriptor alive for the lifetime of the
        // service so the OS does not GC the underlying fd.
        val res = Vpn21Native.nativeStartWithFd(
            profileJson = profileJson,
            fd = fd.fd,
            mtu = 1500,
            ipv4 = TUN_IP,
            mask = TUN_PREFIX,
            dnsPort = 53,
        )
        val ok = runCatching { JSONObject(res).optBoolean("ok") }.getOrDefault(false)
        if (!ok) {
            val errMsg = runCatching { JSONObject(res).optString("error") }.getOrDefault(res)
            pushResult(false, errMsg)
            shutdown()
            return
        }
        pushResult(true, null)
    }

    private fun shutdown() {
        runCatching { Vpn21Native.nativeStop() }
        runCatching { pfd?.close() }
        pfd = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
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
        runCatching { Vpn21Native.nativeStop() }
        runCatching { pfd?.close() }
        pfd = null
    }

    companion object {
        const val ACTION_START = "com.vpn21.app.START"
        const val ACTION_STOP = "com.vpn21.app.STOP"
        const val EXTRA_PROFILE = "profile"
        private const val CHANNEL_ID = "vpn21"
        private const val NOTIF_ID = 1

        private const val TUN_IP = "10.19.21.1"
        private const val TUN_PREFIX = 24

        // Tiny in-process bus used by [MainActivity] to await the start
        // result without an extra IPC layer.
        private var pending: ((Boolean, String?) -> Unit)? = null
        private var latestOk: Boolean? = null
        private var latestErr: String? = null

        fun waitForStart(cb: (Boolean, String?) -> Unit) {
            val ok = latestOk
            val err = latestErr
            if (ok != null || err != null) {
                cb(ok ?: false, err)
                latestOk = null
                latestErr = null
                return
            }
            pending = cb
        }

        fun pushResult(ok: Boolean, err: String?) {
            val p = pending
            if (p != null) {
                p(ok, err)
                pending = null
            } else {
                latestOk = ok
                latestErr = err
            }
        }
    }
}
