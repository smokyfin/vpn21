import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

typedef _Vpn21InitNative = Int32 Function(Pointer<Utf8> appDir, Int32 verbose);
typedef _Vpn21InitDart = int Function(Pointer<Utf8> appDir, int verbose);

typedef _Vpn21StringNative = Pointer<Utf8> Function();
typedef _Vpn21StringDart = Pointer<Utf8> Function();

typedef _Vpn21LogsNative = Pointer<Utf8> Function(Int32 limit);
typedef _Vpn21LogsDart = Pointer<Utf8> Function(int limit);

typedef _Vpn21ParseNative = Pointer<Utf8> Function(
  Pointer<Utf8> id,
  Pointer<Utf8> label,
  Pointer<Utf8> json,
);
typedef _Vpn21ParseDart = Pointer<Utf8> Function(
  Pointer<Utf8> id,
  Pointer<Utf8> label,
  Pointer<Utf8> json,
);

// Desktop-only: Dart hands a profile to Rust, Rust provisions its own TUN
// (utun on macOS, /dev/net/tun on Linux, wintun on Windows) and starts the
// pipeline.  No fd / addressing crosses the Dart boundary.
typedef _Vpn21StartDesktopNative = Pointer<Utf8> Function(Pointer<Utf8> profileJson);
typedef _Vpn21StartDesktopDart = Pointer<Utf8> Function(Pointer<Utf8> profileJson);

typedef _Vpn21FreeNative = Void Function(Pointer<Utf8> s);
typedef _Vpn21FreeDart = void Function(Pointer<Utf8> s);

/// Dart side of the `vpn21-core` FFI.
///
/// **Architectural rule.** The TUN file descriptor never crosses the Dart
/// boundary — Dart only ever knows about a *profile*.  Concretely:
///
///   * On Android the Kotlin `Vpn21VpnService` calls Rust via JNI directly
///     (`Java_com_vpn21_app_Vpn21Native_nativeStartWithFd`) once it has
///     opened the fd from `VpnService.Builder.establish()`.  Dart only
///     dispatches `startVpn` / `stopVpn` MethodChannel calls.
///   * On iOS the `PacketTunnelProvider` extension calls the C-ABI
///     `vpn21_start_with_fd` directly via `Vpn21Bridge` after
///     `setTunnelNetworkSettings` returns.  Dart asks the system to start
///     the extension via `NETunnelProviderManager`.
///   * On desktop (Linux / macOS / Windows) Dart calls
///     `vpn21_start_desktop` via FFI; Rust opens its own TUN.
///
/// This keeps every bit of packet- and credential-handling logic in Rust
/// where the iOS NetworkExtension RAM budget (~50 MB) is realistic.
class Vpn21Native {
  Vpn21Native._();
  static final Vpn21Native instance = Vpn21Native._();

  static const _channel = MethodChannel('vpn21/native');

  DynamicLibrary? _lib;
  late final _Vpn21InitDart _init;
  late final _Vpn21ParseDart _parse;
  late final _Vpn21StringDart _stop;
  late final _Vpn21StringDart _status;
  late final _Vpn21LogsDart _logs;
  late final void Function() _logsClear;
  late final _Vpn21FreeDart _free;
  // Desktop-only entrypoint; null on Android/iOS where Dart never starts
  // the pipeline directly.
  _Vpn21StartDesktopDart? _startDesktop;

  bool _initialised = false;

  DynamicLibrary _open() {
    if (Platform.isAndroid) return DynamicLibrary.open('libvpn21.so');
    if (Platform.isIOS || Platform.isMacOS) return DynamicLibrary.process();
    if (Platform.isLinux) return DynamicLibrary.open('libvpn21.so');
    if (Platform.isWindows) return DynamicLibrary.open('vpn21.dll');
    throw UnsupportedError('platform: ${Platform.operatingSystem}');
  }

  Future<void> init({bool verbose = false}) async {
    if (_initialised) return;
    _lib = _open();
    _init = _lib!.lookupFunction<_Vpn21InitNative, _Vpn21InitDart>('vpn21_init');
    _parse = _lib!.lookupFunction<_Vpn21ParseNative, _Vpn21ParseDart>('vpn21_profile_parse');
    _stop = _lib!.lookupFunction<_Vpn21StringNative, _Vpn21StringDart>('vpn21_stop');
    _status = _lib!.lookupFunction<_Vpn21StringNative, _Vpn21StringDart>('vpn21_status');
    _logs = _lib!.lookupFunction<_Vpn21LogsNative, _Vpn21LogsDart>('vpn21_logs');
    _logsClear = _lib!.lookupFunction<Void Function(), void Function()>('vpn21_logs_clear');
    _free = _lib!.lookupFunction<_Vpn21FreeNative, _Vpn21FreeDart>('vpn21_string_free');
    if (Platform.isLinux || Platform.isMacOS || Platform.isWindows) {
      _startDesktop = _lib!.lookupFunction<_Vpn21StartDesktopNative,
          _Vpn21StartDesktopDart>('vpn21_start_desktop');
    }

    final appDir = await _appDir();
    final pApp = appDir.toNativeUtf8();
    try {
      final rc = _init(pApp, verbose ? 1 : 0);
      if (rc != 0) {
        throw StateError('vpn21_init failed: $rc');
      }
    } finally {
      calloc.free(pApp);
    }
    _initialised = true;
  }

  Future<String> _appDir() async {
    if (Platform.isAndroid || Platform.isIOS) {
      final docs = await getApplicationSupportDirectory();
      return docs.path;
    }
    final base = await getApplicationSupportDirectory();
    return '${base.path}/vpn21';
  }

  Map<String, dynamic> parseProfile({
    required String id,
    required String label,
    required String rawJson,
  }) {
    final a = id.toNativeUtf8();
    final b = label.toNativeUtf8();
    final c = rawJson.toNativeUtf8();
    try {
      final result = _parse(a, b, c);
      final s = result.toDartString();
      _free(result);
      return json.decode(s) as Map<String, dynamic>;
    } finally {
      calloc.free(a);
      calloc.free(b);
      calloc.free(c);
    }
  }

  /// Starts the VPN pipeline for `profile`.
  ///
  /// On Android / iOS Dart asks the platform code to do the heavy lifting:
  /// the platform (Kotlin VpnService / Swift PacketTunnelProvider) opens
  /// the TUN, hands the fd straight into Rust via JNI / direct FFI, and
  /// reports success back through the MethodChannel.  Dart never sees the
  /// fd.
  ///
  /// On desktop Dart calls `vpn21_start_desktop` directly; Rust provisions
  /// the host TUN itself (utun / /dev/net/tun / wintun).
  Future<Map<String, dynamic>> start({
    required Map<String, dynamic> profile,
  }) async {
    final encoded = json.encode(profile);
    if (Platform.isAndroid || Platform.isIOS) {
      final res = await _channel.invokeMethod<Map<dynamic, dynamic>>(
        'startVpn',
        {'profile': encoded},
      );
      return (res ?? {'ok': true, 'detail': 'started'}).cast<String, dynamic>();
    }
    final start = _startDesktop;
    if (start == null) {
      throw StateError('vpn21_start_desktop not available on this platform');
    }
    final pj = encoded.toNativeUtf8();
    try {
      final ptr = start(pj);
      final s = ptr.toDartString();
      _free(ptr);
      return json.decode(s) as Map<String, dynamic>;
    } finally {
      calloc.free(pj);
    }
  }

  Future<Map<String, dynamic>> stop() async {
    if (Platform.isAndroid || Platform.isIOS) {
      // The platform code is responsible for stopping its tunnel service
      // and calling `vpn21_stop` (Kotlin via JNI, Swift via FFI) before
      // returning.  Falling through to the FFI here would race the
      // service teardown.
      final res = await _channel.invokeMethod<Map<dynamic, dynamic>>('stopVpn');
      return (res ?? {'ok': true, 'detail': 'stopped'}).cast<String, dynamic>();
    }
    final ptr = _stop();
    final s = ptr.toDartString();
    _free(ptr);
    return json.decode(s) as Map<String, dynamic>;
  }

  Map<String, dynamic> status() {
    final ptr = _status();
    final s = ptr.toDartString();
    _free(ptr);
    return json.decode(s) as Map<String, dynamic>;
  }

  List<Map<String, dynamic>> logs({int limit = 500}) {
    final ptr = _logs(limit);
    final s = ptr.toDartString();
    _free(ptr);
    final obj = json.decode(s) as Map<String, dynamic>;
    return ((obj['entries'] as List?) ?? [])
        .cast<Map<dynamic, dynamic>>()
        .map((e) => e.cast<String, dynamic>())
        .toList();
  }

  void clearLogs() => _logsClear();
}
