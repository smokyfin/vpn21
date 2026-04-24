import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../bridge/native.dart';

class LogsController extends StateNotifier<List<Map<String, dynamic>>> {
  LogsController() : super(const []) {
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => refresh());
    refresh();
  }

  Timer? _timer;

  void refresh() {
    state = Vpn21Native.instance.logs(limit: 500);
  }

  void clear() {
    Vpn21Native.instance.clearLogs();
    state = const [];
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}

final logsProvider =
    StateNotifierProvider<LogsController, List<Map<String, dynamic>>>(
  (ref) => LogsController(),
);
