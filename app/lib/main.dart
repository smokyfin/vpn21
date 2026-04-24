import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'app.dart';
import 'bridge/native.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await Vpn21Native.instance.init();
  runApp(const ProviderScope(child: Vpn21App()));
}
