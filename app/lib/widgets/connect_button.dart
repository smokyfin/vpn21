import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';

import '../state/vpn_state.dart';

class ConnectButton extends StatelessWidget {
  const ConnectButton({
    super.key,
    required this.status,
    required this.onConnect,
    required this.onDisconnect,
  });

  final VpnStatus status;
  final VoidCallback? onConnect;
  final VoidCallback onDisconnect;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    final isConnected = status.stage == VpnStage.connected;
    final isConnecting = status.stage == VpnStage.bootstrapping ||
        status.stage == VpnStage.connecting;
    final isDisconnecting = status.stage == VpnStage.disconnecting;

    if (isDisconnecting) {
      return FilledButton.tonal(
        onPressed: null,
        style: FilledButton.styleFrom(
          minimumSize: const Size(220, 52),
          backgroundColor: Colors.white.withValues(alpha: 0.08),
        ),
        child: const Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
            SizedBox(width: 12),
            Text('Stopping…'),
          ],
        ),
      );
    }

    if (isConnecting) {
      return OutlinedButton(
        onPressed: onDisconnect,
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(220, 52),
          foregroundColor: c.error,
          side: BorderSide(color: c.error.withValues(alpha: 0.5), width: 1.4),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                color: c.error.withValues(alpha: 0.7),
              ),
            ),
            const SizedBox(width: 12),
            const Text('Cancel', style: TextStyle(letterSpacing: 1.2)),
          ],
        ),
      ).animate().fade(duration: 200.ms);
    }

    if (isConnected) {
      return OutlinedButton(
        onPressed: onDisconnect,
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(220, 52),
          foregroundColor: c.primary,
          side: BorderSide(color: c.primary.withValues(alpha: 0.6), width: 1.4),
        ),
        child: const Text('Disconnect', style: TextStyle(letterSpacing: 1.2)),
      ).animate().fade(duration: 250.ms).slideY(begin: 0.1, end: 0);
    }

    return FilledButton(
      onPressed: onConnect,
      style: FilledButton.styleFrom(
        minimumSize: const Size(220, 52),
        backgroundColor: c.primary,
        foregroundColor: Colors.black,
      ),
      child: const Text('Connect', style: TextStyle(fontSize: 16, letterSpacing: 1.5)),
    ).animate().fade(duration: 250.ms).slideY(begin: 0.1, end: 0);
  }
}
