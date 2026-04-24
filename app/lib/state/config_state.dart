import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:shared_preferences/shared_preferences.dart';

import '../bridge/native.dart';

/// Default URL users see pre-populated in the Import screen.
const kDefaultConfigUrl = 'https://incss.ru/vless.conf';

@immutable
class SavedProfile {
  final String id;
  final String label;
  final Map<String, dynamic> data; // parsed Profile (from Rust)
  const SavedProfile({required this.id, required this.label, required this.data});

  Map<String, dynamic> toJson() => {'id': id, 'label': label, 'data': data};
  factory SavedProfile.fromJson(Map<String, dynamic> j) => SavedProfile(
        id: j['id'] as String,
        label: j['label'] as String,
        data: (j['data'] as Map<dynamic, dynamic>).cast<String, dynamic>(),
      );
}

class ProfilesController extends StateNotifier<List<SavedProfile>> {
  ProfilesController() : super(const []) {
    _load();
  }

  static const _kKey = 'profiles';

  Future<void> _load() async {
    final sp = await SharedPreferences.getInstance();
    final raw = sp.getString(_kKey);
    if (raw == null) return;
    final list = (json.decode(raw) as List)
        .cast<Map<dynamic, dynamic>>()
        .map((m) => SavedProfile.fromJson(m.cast<String, dynamic>()))
        .toList();
    state = list;
  }

  Future<void> _save() async {
    final sp = await SharedPreferences.getInstance();
    await sp.setString(_kKey, json.encode(state.map((p) => p.toJson()).toList()));
  }

  Future<SavedProfile> addFromJson(String rawJson, {String? label}) async {
    final id = 'p${DateTime.now().millisecondsSinceEpoch}';
    final res = Vpn21Native.instance.parseProfile(
      id: id,
      label: label ?? 'Imported profile',
      rawJson: rawJson,
    );
    if (res['ok'] != true) {
      throw FormatException(res['error']?.toString() ?? 'parse failed');
    }
    final p = SavedProfile(
      id: id,
      label: label ?? 'Imported profile',
      data: (res['profile'] as Map<dynamic, dynamic>).cast<String, dynamic>(),
    );
    state = [...state, p];
    await _save();
    return p;
  }

  Future<SavedProfile> addFromUrl(String url, {String? label}) async {
    final resp = await http.get(Uri.parse(url)).timeout(const Duration(seconds: 15));
    if (resp.statusCode != 200) {
      throw HttpException('HTTP ${resp.statusCode}');
    }
    return addFromJson(resp.body, label: label ?? Uri.parse(url).host);
  }

  Future<void> remove(String id) async {
    state = state.where((p) => p.id != id).toList();
    await _save();
  }

  Future<void> rename(String id, String newLabel) async {
    state = [
      for (final p in state)
        if (p.id == id) SavedProfile(id: p.id, label: newLabel, data: p.data) else p,
    ];
    await _save();
  }
}

final profilesProvider =
    StateNotifierProvider<ProfilesController, List<SavedProfile>>(
  (ref) => ProfilesController(),
);

/// The profile id currently selected by the user.  Persisted.
final selectedProfileIdProvider = StateProvider<String?>((ref) => null);

final activeProfileProvider = Provider<Map<String, dynamic>?>((ref) {
  final id = ref.watch(selectedProfileIdProvider);
  final profiles = ref.watch(profilesProvider);
  if (id == null && profiles.isNotEmpty) return profiles.first.data;
  if (id == null) return null;
  for (final p in profiles) {
    if (p.id == id) return p.data;
  }
  return null;
});
