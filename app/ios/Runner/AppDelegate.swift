import UIKit
import Flutter
import NetworkExtension

@main
@objc class AppDelegate: FlutterAppDelegate {
    private var pendingResult: FlutterResult?

    override func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        let controller = window?.rootViewController as! FlutterViewController
        let channel = FlutterMethodChannel(name: "vpn21/native", binaryMessenger: controller.binaryMessenger)
        channel.setMethodCallHandler { [weak self] call, result in
            guard let self = self else { return }
            switch call.method {
            case "requestTun":
                self.requestTun(profile: (call.arguments as? [String: Any])?["profile"] as? String, result: result)
            case "releaseTun":
                self.releaseTun(result: result)
            default:
                result(FlutterMethodNotImplemented)
            }
        }
        GeneratedPluginRegistrant.register(with: self)
        return super.application(application, didFinishLaunchingWithOptions: launchOptions)
    }

    private func requestTun(profile: String?, result: @escaping FlutterResult) {
        NETunnelProviderManager.loadAllFromPreferences { (managers, error) in
            if let error = error { result(FlutterError(code: "LOAD_ERR", message: error.localizedDescription, details: nil)); return }
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
                if let err = err { result(FlutterError(code: "SAVE_ERR", message: err.localizedDescription, details: nil)); return }
                manager.loadFromPreferences { err2 in
                    if let err2 = err2 { result(FlutterError(code: "LOAD2_ERR", message: err2.localizedDescription, details: nil)); return }
                    do {
                        try manager.connection.startVPNTunnel()
                        // The TUN fd lives entirely inside the extension; the
                        // Rust core runs there.  We only return a sentinel fd
                        // of -1 to the Dart side so that it knows it doesn't
                        // own the datapath on iOS.
                        result([
                            "fd": -1,
                            "mtu": 1500,
                            "ipv4": "10.19.21.1",
                            "mask": 24,
                            "dnsPort": 53,
                        ])
                    } catch {
                        result(FlutterError(code: "START_ERR", message: error.localizedDescription, details: nil))
                    }
                }
            }
        }
    }

    private func releaseTun(result: @escaping FlutterResult) {
        NETunnelProviderManager.loadAllFromPreferences { (managers, _) in
            managers?.forEach { $0.connection.stopVPNTunnel() }
            result(nil)
        }
    }
}
