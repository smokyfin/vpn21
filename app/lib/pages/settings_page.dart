import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../state/config_state.dart';

class SettingsPage extends ConsumerWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profiles = ref.watch(profilesProvider);
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: ListView(
        children: [
          const ListTile(
            title: Text('Profiles', style: TextStyle(fontWeight: FontWeight.w600)),
          ),
          if (profiles.isEmpty)
            const ListTile(
              leading: Icon(Icons.info_outline),
              title: Text('No profiles saved'),
              subtitle: Text('Add one from the Import tab.'),
            ),
          for (final p in profiles)
            Dismissible(
              key: ValueKey(p.id),
              background: Container(
                alignment: Alignment.centerRight,
                padding: const EdgeInsets.only(right: 16),
                color: Theme.of(context).colorScheme.error.withValues(alpha: 0.14),
                child: const Icon(Icons.delete_outline_rounded),
              ),
              direction: DismissDirection.endToStart,
              onDismissed: (_) => ref.read(profilesProvider.notifier).remove(p.id),
              child: ListTile(
                leading: const Icon(Icons.vpn_key_outlined),
                title: Text(p.label),
                subtitle: Text(
                  p.data['pt_outbound']?['address'] as String? ?? '',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
          const Divider(),
          const ListTile(
            leading: Icon(Icons.info_outline_rounded),
            title: Text('About'),
            subtitle: Text('vpn21 — Tor with custom VLESS pluggable transport.'),
          ),
          const ListTile(
            leading: Icon(Icons.code_outlined),
            title: Text('GitHub'),
            subtitle: Text('https://github.com/smokyfin/vpn21'),
          ),
        ],
      ),
    );
  }
}
