import 'package:flutter_test/flutter_test.dart';
import 'package:vpn21/state/config_state.dart';

void main() {
  group('SavedProfile', () {
    test('toJson / fromJson round-trip preserves fields', () {
      const p = SavedProfile(
        id: 'p1',
        label: 'My server',
        data: {
          'bridgeRsaId': 'AAAA',
          'bridgeEd25519Id': 'BBBB',
          'dohServer': 'https://dns.google/dns-query',
          'ptOutbound': {
            'address': '1.2.3.4',
            'port': 443,
            'network': 'grpc',
            'security': 'reality',
          },
        },
      );
      final j = p.toJson();
      final p2 = SavedProfile.fromJson(j);
      expect(p2.id, p.id);
      expect(p2.label, p.label);
      expect(p2.data['bridgeRsaId'], 'AAAA');
      expect(p2.data['ptOutbound']['port'], 443);
    });

    test('fromJson tolerates dynamic maps', () {
      final j = {
        'id': 'p2',
        'label': 'Other',
        'data': <dynamic, dynamic>{
          'bridgeRsaId': 'X',
          'ptOutbound': <dynamic, dynamic>{'network': 'grpc'},
        },
      };
      final p = SavedProfile.fromJson(j);
      expect(p.id, 'p2');
      expect(
        (p.data['ptOutbound'] as Map)['network'],
        'grpc',
      );
    });
  });

  group('kDefaultConfigUrl', () {
    test('is the incss.ru bootstrap', () {
      expect(kDefaultConfigUrl, 'https://incss.ru/vless.conf');
    });
  });
}
