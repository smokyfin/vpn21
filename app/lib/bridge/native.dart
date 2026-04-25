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

typedef _Vpn21StartNative = Pointer<Utf8> Function(
  Pointer<Utf8> profileJson,
  Int32 tunFd,
  Int32 mtu,
  Pointer<Utf8> ipv4,
  Int32 mask,
  Int32 dnsPort,
);
typedef _Vpn21StartDart = Pointer<Utf8> Function(
  Pointer<Utf8> profileJson,
  int tunFd,
  int mtu,
  Pointer<Utf8> ipv4,
  int mask,
  int dnsPort,
);

typedef _Vpn21FreeNative = Void Function(Pointer<Utf8> s);
typedef _Vpn21FreeDart = void Function(Pointer<Utf8> s);

/// Dart side of the `vpn21-core` FFI.
///
/// On mobile the TUN fd is obtained from the platform channel first and then
/// forwarded to the native library.  On desktop the process owns the TUN
/// itself and can pass -1 until the platform helper hands one over.
class Vpn21Native {
  Vpn21Native._();
  static final Vpn21Native instance = Vpn21Native._();

  static const _channel = MethodChannel('vpn21/native');

  DynamicLibrary? _lib;
  late final _Vpn21InitDart _init;
  late final _Vpn21ParseDart _parse;
  late final _Vpn21StartDart _start;
  late final _Vpn21StringDart _stop;
  late final _Vpn21StringDart _status;
  late final _Vpn21LogsDart _logs;
  late final void Function() _logsClear;
  late final _Vpn21FreeDart _free;
  // Desktop-only: provisions a real TUN via the Rust core (utun on macOS,
  // /dev/net/tun on Linux, wintun on Windows).  Returns JSON the same shape
  // as the platform channel produces on Android/iOS.
  _Vpn21StringDart? _tunProvision;

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
    _start = _lib!.lookupFunction<_Vpn21StartNative, _Vpn21StartDart>('vpn21_start');
    _stop = _lib!.lookupFunction<_Vpn21StringNative, _Vpn21StringDart>('vpn21_stop');
    _status = _lib!.lookupFunction<_Vpn21StringNative, _Vpn21StringDart>('vpn21_status');
    _logs = _lib!.lookupFunction<_Vpn21LogsNative, _Vpn21LogsDart>('vpn21_logs');
    _logsClear = _lib!.lookupFunction<Void Function(), void Function()>('vpn21_logs_clear');
    _free = _lib!.lookupFunction<_Vpn21FreeNative, _Vpn21FreeDart>('vpn21_string_free');
    if (Platform.isLinux || Platform.isMacOS || Platform.isWindows) {
      try {
        _tunProvision = _lib!
            .lookupFunction<_Vpn21StringNative, _Vpn21StringDart>('vpn21_tun_provision');
      } catch (_) {
        // Built without desktop TUN support; requestTun on desktop will
        // return the platform-channel stub instead.
        _tunProvision = null;
      }
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

  /// Requests the platform to create a TUN and return its fd + addressing.
  /// On iOS this triggers `NEPacketTunnelProvider`.  On Android it asks the
  /// Kotlin `VpnService` for the fd.  On desktop it launches/attaches the
  /// helper process.
  Future<Map<String, dynamic>> requestTun(Map<String, dynamic> profile) async {
    // On desktop we call the Rust TUN helper directly via FFI so that
    // the real platform code (utun / /dev/net/tun / wintun) runs; the
    // platform-channel handlers on those runners only return a stub.
    if ((Platform.isLinux || Platform.isMacOS || Platform.isWindows) &&
        _tunProvision != null) {
      final ptr = _tunProvision!();
      final s = ptr.toDartString();
      _free(ptr);
      final obj = json.decode(s) as Map<String, dynamic>;
      if (obj['ok'] != true) {
        throw StateError('vpn21_tun_provision: ${obj['error']}');
      }
      return (obj['tun'] as Map<dynamic, dynamic>).cast<String, dynamic>();
    }
    final res = await _channel.invokeMethod<Map<dynamic, dynamic>>(
      'requestTun',
      {'profile': json.encode(profile)},
    );
    return (res ?? {}).cast<String, dynamic>();
  }

  Future<void> releaseTun() async {
    if (Platform.isLinux || Platform.isMacOS || Platform.isWindows) {
      // Rust closes the TUN fd inside `vpn21_stop`; nothing to do here.
      return;
    }
    await _channel.invokeMethod('releaseTun');
  }

  Future<Map<String, dynamic>> start({
    required Map<String, dynamic> profile,
    required Map<String, dynamic> tun,
  }) async {
    final pj = json.encode(profile).toNativeUtf8();
    final pIpv4 = (tun['ipv4'] as String? ?? '10.19.21.1').toNativeUtf8();
    try {
      final ptr = _start(
        pj,
        tun['fd'] as int? ?? -1,
        tun['mtu'] as int? ?? 1500,
        pIpv4,
        tun['mask'] as int? ?? 24,
        tun['dnsPort'] as int? ?? 53,
      );
      final s = ptr.toDartString();
      _free(ptr);
      return json.decode(s) as Map<String, dynamic>;
    } finally {
      calloc.free(pj);
      calloc.free(pIpv4);
    }
  }

  Future<Map<String, dynamic>> stop() async {
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
