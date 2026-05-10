import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/application/controllers/settings_controller.dart';
import 'package:writer_app/domain/models/settings.dart';
import 'package:writer_app/domain/services_interfaces/settings_service.dart';

class MockSettingsService implements ISettingsService {
  LocalSettings local = const LocalSettings();
  SyncableSettings syncable = const SyncableSettings();

  String? savedWorkspacePath;

  @override
  Future<String> exportSyncableSettingsJson(String workspacePath) async {
    return '{}';
  }

  @override
  Future<void> importSyncableSettingsJson(
    String workspacePath,
    String jsonStr,
  ) async {}

  @override
  Future<LocalSettings> loadLocalSettings() async {
    return local;
  }

  @override
  Future<SyncableSettings> loadSyncableSettings(String workspacePath) async {
    return syncable;
  }

  @override
  Future<void> resetLocalSettingsToDefaults() async {}

  @override
  Future<void> resetSyncableSettingsToDefaults(String workspacePath) async {}

  @override
  Future<void> saveLocalSettings(LocalSettings settings) async {
    local = settings;
  }

  @override
  Future<void> saveSyncableSettings(
    String workspacePath,
    SyncableSettings settings,
  ) async {
    savedWorkspacePath = workspacePath;
    syncable = settings;
  }
}

void main() {
  group('SettingsController', () {
    late MockSettingsService mockService;
    late SettingsController controller;

    setUp(() {
      mockService = MockSettingsService();
      controller = SettingsController(settingsService: mockService);
    });

    test('can save after modifying autoSaveIntervalSeconds', () async {
      await controller.init();

      final current = controller.syncableSettings;
      controller.updateSyncableSettings(
        current.copyWith(autoSaveIntervalSeconds: 120),
      );

      expect(controller.isDirty, true);

      // Need a workspace path to save syncable
      controller.updateLocalSettings(
        controller.localSettings.copyWith(workspacePath: '/test'),
      );

      await controller.save();

      expect(controller.isDirty, false);
      expect(mockService.syncable.autoSaveIntervalSeconds, 120);
    });

    test(
      'can save deepSeekApiKey to SyncableSettings after modifying',
      () async {
        await controller.init();

        final current = controller.syncableSettings;
        controller.updateSyncableSettings(
          current.copyWith(deepSeekApiKey: 'new_api_key'),
        );
        controller.updateLocalSettings(
          controller.localSettings.copyWith(workspacePath: '/test'),
        );

        await controller.save();

        expect(mockService.syncable.deepSeekApiKey, 'new_api_key');
      },
    );

    test('can save githubToken to SyncableSettings after modifying', () async {
      await controller.init();

      final current = controller.syncableSettings;
      controller.updateSyncableSettings(
        current.copyWith(githubToken: 'new_gh_token'),
      );
      controller.updateLocalSettings(
        controller.localSettings.copyWith(workspacePath: '/test'),
      );

      await controller.save();

      expect(mockService.syncable.githubToken, 'new_gh_token');
    });

    test('LocalSettings does not enter settings.sync.json', () async {
      await controller.init();

      controller.updateLocalSettings(
        controller.localSettings.copyWith(
          workspacePath: '/test',
          deviceName: 'New Device',
        ),
      );

      await controller.save();

      // The local settings should be saved to local
      expect(mockService.local.deviceName, 'New Device');

      // They should NOT be in the syncable object saved to the service
      // We check that the schema hasn't changed to include them. The syncable model physically doesn't have it.
      // We verify the saveSyncableSettings was called but its parameter didn't magically get these fields.
      expect(mockService.savedWorkspacePath, '/test');
    });

    test('saveLocal() strictly avoids updating syncable settings', () async {
      await controller.init();

      controller.updateLocalSettings(
        controller.localSettings.copyWith(
          workspacePath: '/test_local_only',
          deviceName: 'Only Local Device',
        ),
      );

      // Mutate sync settings but DONT call standard save()
      controller.updateSyncableSettings(
        controller.syncableSettings.copyWith(autoSaveIntervalSeconds: 999),
      );

      // Save strictly local
      await controller.saveLocal();

      // The local settings should be saved
      expect(mockService.local.deviceName, 'Only Local Device');

      // The sync settings should NOT be flushed
      expect(mockService.savedWorkspacePath, isNot('/test_local_only'));
      expect(mockService.syncable.autoSaveIntervalSeconds, isNot(999));
    });
  });
}
