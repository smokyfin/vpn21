import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vpn21/state/vpn_state.dart';
import 'package:vpn21/widgets/connect_button.dart';

/// Wraps [child] in a minimal MaterialApp so themed widgets work.
Widget _host(Widget child) => MaterialApp(
      home: Scaffold(body: Center(child: child)),
    );

void main() {
  // Use [pump] with a fixed budget instead of [pumpAndSettle] — the
  // connect/disconnect states show a [CircularProgressIndicator] which
  // animates forever and would otherwise time out the harness.
  Future<void> settle(WidgetTester tester) async {
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
  }

  testWidgets('Idle: shows Connect and fires callback', (tester) async {
    var connected = 0;
    await tester.pumpWidget(_host(
      ConnectButton(
        status: const VpnStatus(),
        onConnect: () => connected++,
        onDisconnect: () {},
      ),
    ));
    await settle(tester);
    expect(find.text('Connect'), findsOneWidget);
    await tester.tap(find.text('Connect'));
    expect(connected, 1);
  });

  testWidgets('Connecting: shows Cancel and fires disconnect',
      (tester) async {
    var disconnected = 0;
    await tester.pumpWidget(_host(
      ConnectButton(
        status: const VpnStatus(stage: VpnStage.connecting, progress: 30),
        onConnect: () {},
        onDisconnect: () => disconnected++,
      ),
    ));
    await settle(tester);
    expect(find.text('Cancel'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    expect(disconnected, 1);
  });

  testWidgets('Connected: shows Disconnect', (tester) async {
    await tester.pumpWidget(_host(
      ConnectButton(
        status: const VpnStatus(stage: VpnStage.connected, progress: 100),
        onConnect: () {},
        onDisconnect: () {},
      ),
    ));
    await settle(tester);
    expect(find.text('Disconnect'), findsOneWidget);
  });

  testWidgets('Disconnecting: shows Stopping and is disabled',
      (tester) async {
    await tester.pumpWidget(_host(
      ConnectButton(
        status: const VpnStatus(stage: VpnStage.disconnecting),
        onConnect: () {},
        onDisconnect: () {},
      ),
    ));
    await settle(tester);
    expect(find.textContaining('Stopping'), findsOneWidget);
    // Tonal button with onPressed:null => disabled
    final btn = tester.widget<FilledButton>(find.byType(FilledButton));
    expect(btn.onPressed, isNull);
  });
}
