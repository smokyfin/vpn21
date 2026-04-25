import 'package:flutter_test/flutter_test.dart';
import 'package:vpn21/state/vpn_state.dart';

void main() {
  group('VpnStatus.fromJson', () {
    test('parses known stages', () {
      expect(
        VpnStatus.fromJson(const {
          'state': 'connected',
          'detail': 'ok',
          'progress': 100,
        }).stage,
        VpnStage.connected,
      );
      expect(
        VpnStatus.fromJson(const {'state': 'bootstrapping'}).stage,
        VpnStage.bootstrapping,
      );
      expect(
        VpnStatus.fromJson(const {'state': 'error'}).stage,
        VpnStage.error,
      );
    });

    test('falls back to idle for unknown stages', () {
      expect(
        VpnStatus.fromJson(const {'state': 'nonsense'}).stage,
        VpnStage.idle,
      );
      expect(VpnStatus.fromJson(const {}).stage, VpnStage.idle);
    });

    test('copyWith only overrides provided fields', () {
      const base = VpnStatus(
        stage: VpnStage.connecting,
        detail: 'hello',
        progress: 42,
      );
      final next = base.copyWith(progress: 77);
      expect(next.stage, VpnStage.connecting);
      expect(next.detail, 'hello');
      expect(next.progress, 77);
    });
  });
}
