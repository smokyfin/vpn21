import 'dart:math' as math;
import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../state/config_state.dart';
import '../state/vpn_state.dart';
import '../widgets/connect_button.dart';
import '../widgets/country_picker.dart';

class HomePage extends ConsumerWidget {
  const HomePage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(vpnControllerProvider);
    final profile = ref.watch(activeProfileProvider);
    final profiles = ref.watch(profilesProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('vpn21'),
        actions: [
          IconButton(
            tooltip: 'Reload status',
            onPressed: () {},
            icon: const Icon(Icons.refresh_rounded),
          ),
        ],
      ),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 18),
          child: Column(
            children: [
              _ProfilePicker(profiles: profiles),
              const Spacer(),
              _StatusRing(status: status),
              const SizedBox(height: 24),
              Text(
                _subtitle(status),
                style: Theme.of(context).textTheme.bodyLarge?.copyWith(
                      color: Colors.white.withOpacity(0.72),
                    ),
                textAlign: TextAlign.center,
              ).animate().fade(duration: 400.ms),
              const SizedBox(height: 32),
              ConnectButton(
                status: status,
                onConnect: profile == null
                    ? null
                    : () => ref.read(vpnControllerProvider.notifier).connect(),
                onDisconnect: () =>
                    ref.read(vpnControllerProvider.notifier).disconnect(),
              ),
              const SizedBox(height: 24),
              const CountryPicker(),
              const Spacer(),
            ],
          ),
        ),
      ),
    );
  }

  String _subtitle(VpnStatus s) {
    switch (s.stage) {
      case VpnStage.idle:
        return 'Tap the shield to connect through Tor.';
      case VpnStage.bootstrapping:
        return 'Bootstrapping — ${s.detail}';
      case VpnStage.connecting:
        return 'Building circuit — ${s.detail}';
      case VpnStage.connected:
        return 'You are protected.';
      case VpnStage.disconnecting:
        return 'Closing circuit…';
      case VpnStage.error:
        return 'Error: ${s.detail}';
    }
  }
}

class _ProfilePicker extends ConsumerWidget {
  const _ProfilePicker({required this.profiles});
  final List<SavedProfile> profiles;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selId = ref.watch(selectedProfileIdProvider);
    if (profiles.isEmpty) {
      return Card(
        child: ListTile(
          leading: const Icon(Icons.info_outline),
          title: const Text('No profile yet'),
          subtitle: const Text('Import one from the Import tab.'),
          onTap: () {},
        ),
      );
    }
    return Card(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
        child: DropdownButtonHideUnderline(
          child: DropdownButton<String>(
            isExpanded: true,
            value: selId ?? profiles.first.id,
            items: [
              for (final p in profiles)
                DropdownMenuItem(value: p.id, child: Text(p.label)),
            ],
            onChanged: (v) => ref.read(selectedProfileIdProvider.notifier).state = v,
          ),
        ),
      ),
    );
  }
}

class _StatusRing extends StatelessWidget {
  const _StatusRing({required this.status});
  final VpnStatus status;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    final progress = status.progress.clamp(0, 100) / 100;
    final size = math.min(MediaQuery.of(context).size.width - 120, 260).toDouble();
    final color = switch (status.stage) {
      VpnStage.connected => c.primary,
      VpnStage.error => c.error,
      _ => c.secondary,
    };
    return SizedBox(
      width: size,
      height: size,
      child: Stack(
        alignment: Alignment.center,
        children: [
          TweenAnimationBuilder<double>(
            tween: Tween(begin: 0, end: progress),
            duration: const Duration(milliseconds: 450),
            builder: (_, v, __) => SizedBox.expand(
              child: CircularProgressIndicator(
                value: status.stage == VpnStage.idle ? 0 : v,
                strokeWidth: 6,
                color: color,
                backgroundColor: Colors.white.withOpacity(0.06),
              ),
            ),
          ),
          Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                _label(status.stage),
                style: Theme.of(context).textTheme.titleLarge?.copyWith(
                      color: color,
                      fontWeight: FontWeight.w700,
                      letterSpacing: 1.5,
                    ),
              ),
              const SizedBox(height: 8),
              Text(
                '${status.progress}%',
                style: Theme.of(context).textTheme.displaySmall?.copyWith(
                      fontWeight: FontWeight.w300,
                    ),
              ),
            ],
          ),
        ],
      ),
    )
        .animate(
          target: status.stage == VpnStage.connected ? 1 : 0,
        )
        .scaleXY(begin: 1.0, end: 1.04, duration: 400.ms, curve: Curves.easeOut);
  }

  String _label(VpnStage s) {
    switch (s) {
      case VpnStage.idle:
        return 'IDLE';
      case VpnStage.bootstrapping:
        return 'BOOTING';
      case VpnStage.connecting:
        return 'DIALING';
      case VpnStage.connected:
        return 'CONNECTED';
      case VpnStage.disconnecting:
        return 'STOPPING';
      case VpnStage.error:
        return 'ERROR';
    }
  }
}
