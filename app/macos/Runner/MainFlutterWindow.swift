import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
    override func awakeFromNib() {
        let controller = FlutterViewController()
        let windowFrame = self.frame
        self.contentViewController = controller
        self.setFrame(windowFrame, display: true)

        // No platform channel for VPN start/stop on macOS: Dart talks to
        // the Rust core directly via dart:ffi (`vpn21_start_desktop`,
        // `vpn21_stop`), and the core opens its own utun interface using
        // `tun::platform::macos::provision`.

        RegisterGeneratedPlugins(registry: controller)
        super.awakeFromNib()
    }
}
