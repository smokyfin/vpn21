import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:google_fonts/google_fonts.dart';

import 'pages/home_page.dart';
import 'pages/import_page.dart';
import 'pages/logs_page.dart';
import 'pages/apps_page.dart';
import 'pages/settings_page.dart';
import 'theme.dart';

class Vpn21App extends ConsumerWidget {
  const Vpn21App({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return MaterialApp(
      title: 'vpn21',
      debugShowCheckedModeBanner: false,
      theme: Vpn21Theme.dark(textTheme: GoogleFonts.interTextTheme()),
      home: const _RootShell(),
    );
  }
}

class _RootShell extends ConsumerStatefulWidget {
  const _RootShell();

  @override
  ConsumerState<_RootShell> createState() => _RootShellState();
}

class _RootShellState extends ConsumerState<_RootShell> {
  int _index = 0;

  static const _pages = <Widget>[
    HomePage(),
    ImportPage(),
    AppsPage(),
    LogsPage(),
    SettingsPage(),
  ];

  static const _labels = <({IconData icon, String label})>[
    (icon: Icons.shield_moon_outlined, label: 'Home'),
    (icon: Icons.cloud_download_outlined, label: 'Import'),
    (icon: Icons.apps_outlined, label: 'Apps'),
    (icon: Icons.terminal_outlined, label: 'Logs'),
    (icon: Icons.settings_outlined, label: 'Settings'),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 220),
        transitionBuilder: (child, animation) => FadeTransition(
          opacity: animation,
          child: SlideTransition(
            position: Tween<Offset>(
              begin: const Offset(0, 0.02),
              end: Offset.zero,
            ).animate(animation),
            child: child,
          ),
        ),
        child: KeyedSubtree(
          key: ValueKey<int>(_index),
          child: _pages[_index],
        ),
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (i) => setState(() => _index = i),
        destinations: [
          for (final d in _labels)
            NavigationDestination(
              icon: Icon(d.icon),
              selectedIcon: Icon(d.icon, color: Theme.of(context).colorScheme.primary),
              label: d.label,
            ),
        ],
      ),
    );
  }
}
