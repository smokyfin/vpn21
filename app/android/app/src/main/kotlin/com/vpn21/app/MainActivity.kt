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
import org.json.JSONObject

class MainActivity : FlutterActivity() {

    private var tunRequest: MethodChannel.Result? = null
    private var pendingProfile: JSONObject? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "vpn21/native")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "requestTun" -> onRequestTun(call.argument<String>("profile"), result)
                    "releaseTun" -> {
                        stopService(Intent(this, Vpn21VpnService::class.java))
                        result.success(null)
                    }
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

    private fun onRequestTun(profileJson: String?, result: MethodChannel.Result) {
        pendingProfile = profileJson?.let { JSONObject(it) }
        tunRequest = result
        val prep = VpnService.prepare(this)
        if (prep != null) {
            startActivityForResult(prep, REQ_VPN)
        } else {
            onVpnPermissionGranted()
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQ_VPN) {
            if (resultCode == Activity.RESULT_OK) {
                onVpnPermissionGranted()
            } else {
                tunRequest?.error("VPN_DENIED", "User denied VPN permission", null)
                tunRequest = null
            }
        }
    }

    private fun onVpnPermissionGranted() {
        val intent = Intent(this, Vpn21VpnService::class.java).apply {
            action = Vpn21VpnService.ACTION_START
            putExtra(Vpn21VpnService.EXTRA_PROFILE, pendingProfile?.toString())
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
        // The service publishes the fd + addressing back via a broadcast; we
        // listen and forward it to Dart through [tunRequest].
        Vpn21VpnService.waitForTun { tun, err ->
            if (err != null) {
                tunRequest?.error("TUN_ERR", err, null)
            } else {
                tunRequest?.success(tun)
            }
            tunRequest = null
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
