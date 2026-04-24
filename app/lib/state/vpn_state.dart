import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../bridge/native.dart';
import '../widgets/country_picker.dart';
import 'config_state.dart';

enum VpnStage { idle, bootstrapping, connecting, connected, disconnecting, error }

@immutable
class VpnStatus {
  final VpnStage stage;
  final String detail;
  final int progress;

  const VpnStatus({
    this.stage = VpnStage.idle,
    this.detail = '',
    this.progress = 0,
  });

  VpnStatus copyWith({VpnStage? stage, String? detail, int? progress}) =>
      VpnStatus(
        stage: stage ?? this.stage,
        detail: detail ?? this.detail,
        progress: progress ?? this.progress,
      );

  static VpnStage _parse(String s) => switch (s) {
        'bootstrapping' => VpnStage.bootstrapping,
        'connecting' => VpnStage.connecting,
        'connected' => VpnStage.connected,
        'disconnecting' => VpnStage.disconnecting,
        'error' => VpnStage.error,
        _ => VpnStage.idle,
      };

  factory VpnStatus.fromJson(Map<String, dynamic> j) => VpnStatus(
        stage: _parse(j['state'] as String? ?? 'idle'),
        detail: j['detail'] as String? ?? '',
        progress: (j['progress'] as num?)?.toInt() ?? 0,
      );
}

class VpnController extends StateNotifier<VpnStatus> {
  VpnController(this._ref) : super(const VpnStatus()) {
    _startPolling();
  }

  final Ref _ref;
  Timer? _poll;

  void _startPolling() {
    _poll?.cancel();
    _poll = Timer.periodic(const Duration(milliseconds: 750), (_) {
      final s = VpnStatus.fromJson(Vpn21Native.instance.status());
      if (s.stage != state.stage ||
          s.detail != state.detail ||
          s.progress != state.progress) {
        state = s;
      }
    });
  }

  Future<void> connect() async {
    final profile = _ref.read(activeProfileProvider);
    if (profile == null) {
      state = state.copyWith(stage: VpnStage.error, detail: 'no profile');
      return;
    }
    // Apply the user's exit-country selection (if any) to the profile just
    // before we hand it to the Rust core.  Null / empty means "any country".
    final country = _ref.read(selectedCountryProvider);
    final profileWithCountry = <String, dynamic>{
      ...profile,
      'exit_country': country ?? '',
    };
    state = state.copyWith(stage: VpnStage.bootstrapping, detail: 'requesting tun', progress: 2);
    try {
      final tun = await Vpn21Native.instance.requestTun(profileWithCountry);
      await Vpn21Native.instance.start(profile: profileWithCountry, tun: tun);
    } catch (e) {
      state = state.copyWith(stage: VpnStage.error, detail: '$e');
    }
  }

  Future<void> disconnect() async {
    state = state.copyWith(stage: VpnStage.disconnecting, detail: 'stopping', progress: 40);
    try {
      await Vpn21Native.instance.stop();
      await Vpn21Native.instance.releaseTun();
    } catch (e) {
      state = state.copyWith(stage: VpnStage.error, detail: '$e');
    }
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }
}

final vpnControllerProvider =
    StateNotifierProvider<VpnController, VpnStatus>((ref) => VpnController(ref));
