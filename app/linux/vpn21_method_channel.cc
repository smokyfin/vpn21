// Intentionally empty.
//
// On Linux the Dart side talks to the Rust core directly via dart:ffi
// (`vpn21_start_desktop`, `vpn21_stop`), and the core opens its own TUN
// device using `tun::platform::linux::provision`.  No Flutter method
// channel is required.
//
// The translation unit is kept (rather than removed from CMakeLists) so
// that incremental rebuilds of older trees still find a target.

#include <flutter_linux/flutter_linux.h>

void vpn21_register_method_channel(FlBinaryMessenger* /*messenger*/) {
    // no-op: see header comment
}
