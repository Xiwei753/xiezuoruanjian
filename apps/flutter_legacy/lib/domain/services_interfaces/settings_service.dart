import '../models/settings.dart';

abstract class ISettingsService {
  Future<LocalSettings> loadLocalSettings();
  Future<void> saveLocalSettings(LocalSettings settings);
  Future<SyncableSettings> loadSyncableSettings(String workspacePath);
  Future<void> saveSyncableSettings(
    String workspacePath,
    SyncableSettings settings,
  );
  Future<void> resetSyncableSettingsToDefaults(String workspacePath);
  Future<void> resetLocalSettingsToDefaults();
  Future<String> exportSyncableSettingsJson(String workspacePath);
  Future<void> importSyncableSettingsJson(String workspacePath, String jsonStr);
}
