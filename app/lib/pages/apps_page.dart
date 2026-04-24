import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

const _channel = MethodChannel('vpn21/android_apps');

class InstalledApp {
  final String packageName;
  final String appName;
  final bool system;
  const InstalledApp({required this.packageName, required this.appName, required this.system});

  factory InstalledApp.fromMap(Map<dynamic, dynamic> m) => InstalledApp(
        packageName: m['pkg']?.toString() ?? '',
        appName: m['name']?.toString() ?? '',
        system: m['system'] == true,
      );
}

enum AppListMode { disabled, included, excluded }

final appModeProvider = StateProvider<AppListMode>((ref) => AppListMode.disabled);
final appSelectionProvider = StateProvider<Set<String>>((ref) => {});

class AppsPage extends ConsumerStatefulWidget {
  const AppsPage({super.key});

  @override
  ConsumerState<AppsPage> createState() => _AppsPageState();
}

class _AppsPageState extends ConsumerState<AppsPage> {
  List<InstalledApp> _apps = [];
  bool _loading = false;
  String _filter = '';

  @override
  void initState() {
    super.initState();
    _loadPrefs();
    if (Platform.isAndroid) _load();
  }

  Future<void> _loadPrefs() async {
    final sp = await SharedPreferences.getInstance();
    final mode = sp.getString('apps_mode') ?? 'disabled';
    final sel = (sp.getStringList('apps_sel') ?? []).toSet();
    ref.read(appModeProvider.notifier).state = AppListMode.values.firstWhere(
      (m) => m.name == mode,
      orElse: () => AppListMode.disabled,
    );
    ref.read(appSelectionProvider.notifier).state = sel;
  }

  Future<void> _savePrefs() async {
    final sp = await SharedPreferences.getInstance();
    await sp.setString('apps_mode', ref.read(appModeProvider).name);
    await sp.setStringList('apps_sel', ref.read(appSelectionProvider).toList());
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final raw = await _channel.invokeMethod<List<dynamic>>('list');
      setState(() {
        _apps = (raw ?? [])
            .cast<Map<dynamic, dynamic>>()
            .map(InstalledApp.fromMap)
            .toList()
          ..sort((a, b) => a.appName.toLowerCase().compareTo(b.appName.toLowerCase()));
      });
    } on PlatformException {
      // Platform not yet plumbed — fall back to empty list.
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final mode = ref.watch(appModeProvider);
    final sel = ref.watch(appSelectionProvider);

    if (!Platform.isAndroid) {
      return Scaffold(
        appBar: AppBar(title: const Text('Per-app VPN')),
        body: const Center(
          child: Padding(
            padding: EdgeInsets.all(24),
            child: Text(
              'Per-app VPN rules are only available on Android.',
              textAlign: TextAlign.center,
            ),
          ),
        ),
      );
    }

    final filtered = _apps.where((a) {
      if (_filter.isEmpty) return true;
      final f = _filter.toLowerCase();
      return a.appName.toLowerCase().contains(f) || a.packageName.contains(f);
    }).toList();

    return Scaffold(
      appBar: AppBar(
        title: const Text('Per-app VPN'),
        actions: [
          IconButton(onPressed: _load, icon: const Icon(Icons.refresh_rounded)),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: SegmentedButton<AppListMode>(
              segments: const [
                ButtonSegment(value: AppListMode.disabled, label: Text('All apps')),
                ButtonSegment(value: AppListMode.included, label: Text('Only selected')),
                ButtonSegment(value: AppListMode.excluded, label: Text('Exclude selected')),
              ],
              selected: {mode},
              onSelectionChanged: (s) {
                ref.read(appModeProvider.notifier).state = s.first;
                _savePrefs();
              },
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
            child: TextField(
              decoration: const InputDecoration(
                prefixIcon: Icon(Icons.search_rounded),
                hintText: 'Filter apps',
              ),
              onChanged: (v) => setState(() => _filter = v),
            ),
          ),
          if (_loading) const LinearProgressIndicator(),
          Expanded(
            child: ListView.builder(
              itemCount: filtered.length,
              itemBuilder: (_, i) {
                final a = filtered[i];
                final on = sel.contains(a.packageName);
                return SwitchListTile(
                  title: Text(a.appName),
                  subtitle: Text(a.packageName),
                  value: on,
                  onChanged: (v) {
                    final next = Set<String>.of(sel);
                    if (v) {
                      next.add(a.packageName);
                    } else {
                      next.remove(a.packageName);
                    }
                    ref.read(appSelectionProvider.notifier).state = next;
                    _savePrefs();
                  },
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}
