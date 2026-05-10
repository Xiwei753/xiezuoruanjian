import 'dart:convert';
import 'dart:io';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

import '../../domain/models/settings.dart';
import '../../domain/services_interfaces/settings_service.dart';
import '../../domain/services_interfaces/storage_service.dart';
import '../storage/atomic_writer.dart';

class SettingsServiceImpl implements ISettingsService {
  final IStorageService _storageService;

  SettingsServiceImpl({IStorageService? storageService})
    : _storageService = storageService ?? AtomicWriter();

  Future<File> _getLocalSettingsFile() async {
    final supportDir = await getApplicationSupportDirectory();
    final file = File(p.join(supportDir.path, 'settings.local.json'));
    return file;
  }

  File _getSyncableSettingsFile(String workspacePath) {
    return File(
      p.join(workspacePath, 'app-meta', 'settings', 'settings.sync.json'),
    );
  }

  @override
  Future<LocalSettings> loadLocalSettings() async {
    try {
      final file = await _getLocalSettingsFile();
      if (!await file.exists()) {
        return const LocalSettings();
      }
      final content = await file.readAsString();
      final json = jsonDecode(content) as Map<String, dynamic>;
      return LocalSettings.fromJson(json);
    } catch (e) {
      // Graceful fallback to defaults on corruption
      return const LocalSettings();
    }
  }

  @override
  Future<void> saveLocalSettings(LocalSettings settings) async {
    final file = await _getLocalSettingsFile();
    final jsonStr = const JsonEncoder.withIndent(
      '  ',
    ).convert(settings.toJson());
    // Use atomic writer for safety if we wanted to, but since it requires IStorageService interface,
    // let's just use it properly. Note that we write to a global path, outside the workspace.
    await _storageService.atomicWrite(file.path, jsonStr);
  }

  @override
  Future<SyncableSettings> loadSyncableSettings(String workspacePath) async {
    try {
      final file = _getSyncableSettingsFile(workspacePath);
      if (!await file.exists()) {
        final defaults = const SyncableSettings();
        await saveSyncableSettings(workspacePath, defaults);
        return defaults;
      }
      final content = await file.readAsString();
      final json = jsonDecode(content) as Map<String, dynamic>;
      return SyncableSettings.fromJson(json);
    } catch (e) {
      return const SyncableSettings();
    }
  }

  @override
  Future<void> saveSyncableSettings(
    String workspacePath,
    SyncableSettings settings,
  ) async {
    final file = _getSyncableSettingsFile(workspacePath);
    final jsonStr = const JsonEncoder.withIndent(
      '  ',
    ).convert(settings.toJson());
    await _storageService.atomicWrite(file.path, jsonStr);
  }

  @override
  Future<void> resetLocalSettingsToDefaults() async {
    await saveLocalSettings(const LocalSettings());
  }

  @override
  Future<void> resetSyncableSettingsToDefaults(String workspacePath) async {
    await saveSyncableSettings(workspacePath, const SyncableSettings());
  }

  @override
  Future<String> exportSyncableSettingsJson(String workspacePath) async {
    final file = _getSyncableSettingsFile(workspacePath);
    if (!await file.exists()) {
      return const JsonEncoder.withIndent(
        '  ',
      ).convert(const SyncableSettings().toJson());
    }
    return await file.readAsString();
  }

  @override
  Future<void> importSyncableSettingsJson(
    String workspacePath,
    String jsonStr,
  ) async {
    try {
      final json = jsonDecode(jsonStr) as Map<String, dynamic>;
      final settings = SyncableSettings.fromJson(json);
      await saveSyncableSettings(workspacePath, settings);
    } catch (e) {
      throw Exception('Invalid JSON format for settings import');
    }
  }
}
