import 'dart:io';

import 'package:flutter/services.dart';

/// Desktop (Linux, macOS, Windows) platform handler for TUN management.
///
/// On desktop the Rust core manages TUN setup directly. The Dart side only
/// needs to tell the native library "start" with a profile and collect the
/// resulting TUN metadata.
///
/// Unlike Android (which uses a `VpnService`) and iOS (which uses a
/// `PacketTunnelProvider`), desktop platforms let the process create TUN
/// devices directly (with elevated privileges or a helper).
class DesktopPlatform {
  DesktopPlatform._();
  static final DesktopPlatform instance = DesktopPlatform._();

  static const _channel = MethodChannel('vpn21/native');

  static bool get isDesktop =>
      Platform.isLinux || Platform.isMacOS || Platform.isWindows;

  /// Requests TUN creation on desktop. The platform native side (C++/Swift)
  /// creates the TUN device and returns its metadata.
  Future<Map<String, dynamic>> requestTun(Map<String, dynamic> profile) async {
    if (!isDesktop) {
      throw UnsupportedError('DesktopPlatform.requestTun on ${Platform.operatingSystem}');
    }
    try {
      final res = await _channel.invokeMethod<Map<dynamic, dynamic>>(
        'requestTun',
        {'profile': profile},
      );
      return (res ?? _fallbackTun()).cast<String, dynamic>();
    } on MissingPluginException {
      // Platform channel not yet wired: return a stub so the UI doesn't crash.
      return _fallbackTun();
    }
  }

  Future<void> releaseTun() async {
    try {
      await _channel.invokeMethod('releaseTun');
    } on MissingPluginException {
      // Not yet implemented on this platform.
    }
  }

  static Map<String, dynamic> _fallbackTun() => {
        'fd': -1,
        'mtu': 1500,
        'ipv4': '10.19.21.1',
        'mask': 24,
        'dnsPort': 53,
      };
}
