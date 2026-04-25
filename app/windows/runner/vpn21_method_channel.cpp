// Intentionally empty.
//
// On Windows the Dart side talks to the Rust core directly via dart:ffi
// (`vpn21_start_desktop`, `vpn21_stop`), and the core uses the WinTun
// driver via `tun::platform::windows::provision`.  No Flutter method
// channel is required.

#include <flutter/binary_messenger.h>

void Vpn21RegisterMethodChannel(flutter::BinaryMessenger* /*messenger*/) {
    // no-op: see header comment
}
