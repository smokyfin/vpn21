import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// A minimal country picker — the full list of Tor exits is too long for a
/// prominent control, so we surface the most commonly requested ones first
/// and expose "Any" as a null-country fallback.  The selection is persisted
/// in the Riverpod state and forwarded to the Rust layer at `start()` time.
final selectedCountryProvider = StateProvider<String?>((ref) => null);

class CountryPicker extends ConsumerWidget {
  const CountryPicker({super.key});

  static const _popular = <({String code, String label, String flag})>[
    (code: '', label: 'Any', flag: '🌐'),
    (code: 'NL', label: 'Netherlands', flag: '🇳🇱'),
    (code: 'DE', label: 'Germany', flag: '🇩🇪'),
    (code: 'FR', label: 'France', flag: '🇫🇷'),
    (code: 'SE', label: 'Sweden', flag: '🇸🇪'),
    (code: 'CH', label: 'Switzerland', flag: '🇨🇭'),
    (code: 'US', label: 'United States', flag: '🇺🇸'),
    (code: 'CA', label: 'Canada', flag: '🇨🇦'),
    (code: 'GB', label: 'United Kingdom', flag: '🇬🇧'),
    (code: 'JP', label: 'Japan', flag: '🇯🇵'),
    (code: 'SG', label: 'Singapore', flag: '🇸🇬'),
    (code: 'AU', label: 'Australia', flag: '🇦🇺'),
  ];

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(selectedCountryProvider);
    return Card(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 4),
        child: DropdownButtonHideUnderline(
          child: DropdownButton<String>(
            isExpanded: true,
            value: selected ?? '',
            items: [
              for (final c in _popular)
                DropdownMenuItem(
                  value: c.code,
                  child: Row(
                    children: [
                      Text(c.flag, style: const TextStyle(fontSize: 20)),
                      const SizedBox(width: 12),
                      Text('Exit: ${c.label}'),
                    ],
                  ),
                ),
            ],
            onChanged: (v) =>
                ref.read(selectedCountryProvider.notifier).state = v == '' ? null : v,
          ),
        ),
      ),
    );
  }
}
