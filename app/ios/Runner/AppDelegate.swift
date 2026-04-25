import UIKit
import Flutter
import NetworkExtension

@main
@objc class AppDelegate: FlutterAppDelegate {
    override func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        let controller = window?.rootViewController as! FlutterViewController
        let channel = FlutterMethodChannel(name: "vpn21/native", binaryMessenger: controller.binaryMessenger)
        channel.setMethodCallHandler { [weak self] call, result in
            guard let self = self else { return }
            switch call.method {
            case "startVpn":
                let profile = (call.arguments as? [String: Any])?["profile"] as? String
                self.startVpn(profile: profile, result: result)
            case "stopVpn":
                self.stopVpn(result: result)
            default:
                result(FlutterMethodNotImplemented)
            }
        }
        GeneratedPluginRegistrant.register(with: self)
        return super.application(application, didFinishLaunchingWithOptions: launchOptions)
    }

    /// Starts the PacketTunnel extension.  Once the extension is up the
    /// Rust core (linked into the extension as a static library) calls
    /// `vpn21_start_with_fd` with the in-process packet flow — Dart never
    /// sees a fd.
    private func startVpn(profile: String?, result: @escaping FlutterResult) {
        NETunnelProviderManager.loadAllFromPreferences { (managers, error) in
            if let error = error {
                result(FlutterError(code: "LOAD_ERR", message: error.localizedDescription, details: nil))
                return
            }
            let manager = managers?.first ?? NETunnelProviderManager()
            let proto = NETunnelProviderProtocol()
            proto.providerBundleIdentifier = "com.vpn21.app.PacketTunnel"
            proto.serverAddress = "vpn21"
            if let profile = profile {
                proto.providerConfiguration = ["profile": profile]
            }
            manager.protocolConfiguration = proto
            manager.localizedDescription = "vpn21"
            manager.isEnabled = true
            manager.saveToPreferences { err in
                if let err = err {
                    result(FlutterError(code: "SAVE_ERR", message: err.localizedDescription, details: nil))
                    return
                }
                manager.loadFromPreferences { err2 in
                    if let err2 = err2 {
                        result(FlutterError(code: "LOAD2_ERR", message: err2.localizedDescription, details: nil))
                        return
                    }
                    do {
                        try manager.connection.startVPNTunnel()
                        result(["ok": true, "detail": "started"])
                    } catch {
                        result(FlutterError(code: "START_ERR", message: error.localizedDescription, details: nil))
                    }
                }
            }
        }
    }

    private func stopVpn(result: @escaping FlutterResult) {
        NETunnelProviderManager.loadAllFromPreferences { (managers, _) in
            managers?.forEach { $0.connection.stopVPNTunnel() }
            result(["ok": true, "detail": "stopped"])
        }
    }
}
