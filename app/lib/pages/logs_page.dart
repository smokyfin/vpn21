import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';

import '../state/logs_state.dart';
import '../state/vpn_state.dart';

class LogsPage extends ConsumerWidget {
  const LogsPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final logs = ref.watch(logsProvider);
    final status = ref.watch(vpnControllerProvider);
    final df = DateFormat('HH:mm:ss.SSS');
    return Scaffold(
      appBar: AppBar(
        title: const Text('Logs & status'),
        actions: [
          IconButton(
            tooltip: 'Refresh',
            onPressed: () => ref.read(logsProvider.notifier).refresh(),
            icon: const Icon(Icons.refresh_rounded),
          ),
          IconButton(
            tooltip: 'Clear',
            onPressed: () => ref.read(logsProvider.notifier).clear(),
            icon: const Icon(Icons.delete_outline_rounded),
          ),
        ],
      ),
      body: Column(
        children: [
          _StatusCard(status: status),
          const Divider(height: 1),
          Expanded(
            child: logs.isEmpty
                ? const _Empty()
                : ListView.separated(
                    reverse: true,
                    itemCount: logs.length,
                    separatorBuilder: (_, __) => const Divider(height: 1, thickness: 0.2),
                    itemBuilder: (_, i) {
                      final e = logs[i];
                      final ts = e['ts_ms'] as int? ?? 0;
                      final level = (e['level'] as String? ?? 'INFO').toUpperCase();
                      final msg = e['msg'] as String? ?? '';
                      final color = switch (level) {
                        'ERROR' => Colors.redAccent,
                        'WARN' => Colors.amber,
                        'DEBUG' => Colors.blueGrey,
                        'TRACE' => Colors.grey,
                        _ => Colors.white.withValues(alpha: 0.75),
                      };
                      return Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
                        child: RichText(
                          text: TextSpan(
                            style: const TextStyle(
                              fontFamily: 'monospace',
                              fontSize: 12,
                              color: Colors.white,
                            ),
                            children: [
                              TextSpan(
                                text: df.format(DateTime.fromMillisecondsSinceEpoch(ts)) + '  ',
                                style: TextStyle(color: Colors.white.withValues(alpha: 0.45)),
                              ),
                              TextSpan(
                                text: level.padRight(6),
                                style: TextStyle(color: color, fontWeight: FontWeight.w700),
                              ),
                              TextSpan(text: msg, style: TextStyle(color: color)),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({required this.status});
  final VpnStatus status;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    final color = switch (status.stage) {
      VpnStage.connected => c.primary,
      VpnStage.error => c.error,
      VpnStage.idle => Colors.white70,
      _ => c.secondary,
    };
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 10, 16, 12),
      child: Row(
        children: [
          Container(
            width: 10,
            height: 10,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              '${status.stage.name.toUpperCase()} — ${status.detail.isEmpty ? "…" : status.detail}',
              style: const TextStyle(fontWeight: FontWeight.w600),
            ),
          ),
          Text('${status.progress}%'),
        ],
      ),
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty();

  @override
  Widget build(BuildContext context) => Center(
        child: Text(
          'No log entries yet.',
          style: TextStyle(color: Colors.white.withValues(alpha: 0.4)),
        ),
      );
}
