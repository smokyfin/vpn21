package com.vpn21.app

import android.app.Activity
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/**
 * Hosts the Flutter engine and routes the two MethodChannels the Dart
 * bridge talks to:
 *
 *   * `vpn21/native#startVpn` / `stopVpn` — orchestrate the foreground
 *     VpnService.  The TUN fd never reaches Dart; the service hands it
 *     straight to Rust via JNI.
 *   * `vpn21/android_apps#list` — returns the installed app list for the
 *     per-app filter UI.
 */
class MainActivity : FlutterActivity() {

    private var startResult: MethodChannel.Result? = null
    private var pendingProfileJson: String? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "vpn21/native")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "startVpn" -> onStartVpn(call.argument<String>("profile"), result)
                    "stopVpn" -> onStopVpn(result)
                    else -> result.notImplemented()
                }
            }

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "vpn21/android_apps")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "list" -> result.success(listInstalledApps())
                    else -> result.notImplemented()
                }
            }
    }

    private fun onStartVpn(profileJson: String?, result: MethodChannel.Result) {
        pendingProfileJson = profileJson
        startResult = result
        val prep = VpnService.prepare(this)
        if (prep != null) {
            startActivityForResult(prep, REQ_VPN)
        } else {
            launchService()
        }
    }

    private fun onStopVpn(result: MethodChannel.Result) {
        val intent = Intent(this, Vpn21VpnService::class.java).apply {
            action = Vpn21VpnService.ACTION_STOP
        }
        startService(intent)
        result.success(mapOf("ok" to true, "detail" to "stopped"))
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQ_VPN) {
            if (resultCode == Activity.RESULT_OK) {
                launchService()
            } else {
                startResult?.error("VPN_DENIED", "User denied VPN permission", null)
                startResult = null
            }
        }
    }

    private fun launchService() {
        val intent = Intent(this, Vpn21VpnService::class.java).apply {
            action = Vpn21VpnService.ACTION_START
            putExtra(Vpn21VpnService.EXTRA_PROFILE, pendingProfileJson ?: "")
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
        // The service publishes the start result back via a tiny in-process
        // bus; we forward it to Dart so the UI can transition out of the
        // "starting" stage.
        Vpn21VpnService.waitForStart { ok, err ->
            val r = startResult
            startResult = null
            if (r == null) return@waitForStart
            if (ok) {
                r.success(mapOf("ok" to true, "detail" to "started"))
            } else {
                r.error("START_FAIL", err ?: "unknown error", null)
            }
        }
    }

    private fun listInstalledApps(): List<Map<String, Any?>> {
        val pm = packageManager
        val apps = pm.getInstalledApplications(PackageManager.GET_META_DATA)
        return apps.map {
            mapOf(
                "pkg" to it.packageName,
                "name" to pm.getApplicationLabel(it).toString(),
                "system" to ((it.flags and ApplicationInfo.FLAG_SYSTEM) != 0),
            )
        }
    }

    companion object {
        private const val REQ_VPN = 7321
    }
}
