import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
    override func awakeFromNib() {
        let controller = FlutterViewController()
        let windowFrame = self.frame
        self.contentViewController = controller
        self.setFrame(windowFrame, display: true)

        let channel = FlutterMethodChannel(
            name: "vpn21/native",
            binaryMessenger: controller.engine.binaryMessenger
        )
        channel.setMethodCallHandler { call, result in
            switch call.method {
            case "requestTun":
                // On macOS the TUN is managed by a System Extension (or the
                // Rust core directly if running with elevated privileges).
                // Return a sentinel fd of -1 so the Dart side knows it does
                // not own the TUN.
                result([
                    "fd": -1,
                    "mtu": 1500,
                    "ipv4": "10.19.21.1",
                    "mask": 24,
                    "dnsPort": 53,
                ] as [String: Any])
            case "releaseTun":
                result(nil)
            default:
                result(FlutterMethodNotImplemented)
            }
        }

        RegisterGeneratedPlugins(registry: controller)
        super.awakeFromNib()
    }
}
