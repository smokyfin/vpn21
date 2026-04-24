import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../state/config_state.dart';

class ImportPage extends ConsumerStatefulWidget {
  const ImportPage({super.key});

  @override
  ConsumerState<ImportPage> createState() => _ImportPageState();
}

class _ImportPageState extends ConsumerState<ImportPage> {
  final _urlCtrl = TextEditingController(text: kDefaultConfigUrl);
  final _labelCtrl = TextEditingController();
  final _pasteCtrl = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _urlCtrl.dispose();
    _labelCtrl.dispose();
    _pasteCtrl.dispose();
    super.dispose();
  }

  Future<void> _importUrl() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref
          .read(profilesProvider.notifier)
          .addFromUrl(_urlCtrl.text, label: _labelCtrl.text.isEmpty ? null : _labelCtrl.text);
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Profile imported.')));
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _importPaste() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref
          .read(profilesProvider.notifier)
          .addFromJson(_pasteCtrl.text, label: _labelCtrl.text.isEmpty ? null : _labelCtrl.text);
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Profile imported.')));
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _scanQr() async {
    final code = await Navigator.of(context).push<String>(
      MaterialPageRoute(builder: (_) => const _QrScannerPage()),
    );
    if (code == null) return;
    if (code.startsWith('http')) {
      _urlCtrl.text = code;
      await _importUrl();
    } else {
      _pasteCtrl.text = code;
      await _importPaste();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Import profile')),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _labelCtrl,
              decoration: const InputDecoration(labelText: 'Label (optional)'),
            ),
            const SizedBox(height: 20),
            _Section(title: 'From URL', child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextField(
                  controller: _urlCtrl,
                  decoration: const InputDecoration(labelText: 'Profile URL'),
                ),
                const SizedBox(height: 10),
                FilledButton.icon(
                  onPressed: _busy ? null : _importUrl,
                  icon: const Icon(Icons.cloud_download_outlined),
                  label: const Text('Fetch & import'),
                ),
              ],
            )),
            const SizedBox(height: 16),
            _Section(title: 'Paste JSON', child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextField(
                  controller: _pasteCtrl,
                  minLines: 5,
                  maxLines: 10,
                  decoration: const InputDecoration(
                    labelText: 'Config JSON',
                    hintText: '{ ... }',
                  ),
                ),
                const SizedBox(height: 10),
                FilledButton.icon(
                  onPressed: _busy ? null : _importPaste,
                  icon: const Icon(Icons.paste_rounded),
                  label: const Text('Import pasted'),
                ),
              ],
            )),
            const SizedBox(height: 16),
            _Section(title: 'Scan QR code', child: FilledButton.icon(
              onPressed: _busy ? null : _scanQr,
              icon: const Icon(Icons.qr_code_scanner_rounded),
              label: const Text('Open camera'),
            )),
            if (_error != null) ...[
              const SizedBox(height: 16),
              Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ).animate().shake(hz: 4, duration: 300.ms),
            ],
          ],
        ),
      ),
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child});
  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            child,
          ],
        ),
      ),
    );
  }
}

class _QrScannerPage extends StatefulWidget {
  const _QrScannerPage();

  @override
  State<_QrScannerPage> createState() => _QrScannerPageState();
}

class _QrScannerPageState extends State<_QrScannerPage> {
  final MobileScannerController _controller = MobileScannerController();
  bool _popped = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Scan QR')),
      body: MobileScanner(
        controller: _controller,
        onDetect: (capture) {
          if (_popped) return;
          final barcodes = capture.barcodes;
          for (final b in barcodes) {
            final v = b.rawValue;
            if (v != null && v.isNotEmpty) {
              _popped = true;
              Navigator.of(context).pop(v);
              return;
            }
          }
        },
      ),
    );
  }
}
