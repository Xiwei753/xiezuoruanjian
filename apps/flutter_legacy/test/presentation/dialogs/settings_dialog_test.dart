import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/application/controllers/settings_controller.dart';
import 'package:writer_app/domain/models/settings.dart';
import 'package:writer_app/presentation/dialogs/settings_dialog.dart';
import 'package:writer_app/domain/services_interfaces/settings_service.dart';

class MockSettingsService implements ISettingsService {
  LocalSettings local = const LocalSettings();
  SyncableSettings syncable = const SyncableSettings();

  @override
  Future<String> exportSyncableSettingsJson(String workspacePath) async => '{}';
  @override
  Future<void> importSyncableSettingsJson(
    String workspacePath,
    String jsonStr,
  ) async {}
  @override
  Future<LocalSettings> loadLocalSettings() async => local;
  @override
  Future<SyncableSettings> loadSyncableSettings(String workspacePath) async =>
      syncable;
  @override
  Future<void> resetLocalSettingsToDefaults() async {}
  @override
  Future<void> resetSyncableSettingsToDefaults(String workspacePath) async {}
  @override
  Future<void> saveLocalSettings(LocalSettings settings) async =>
      local = settings;
  @override
  Future<void> saveSyncableSettings(
    String workspacePath,
    SyncableSettings settings,
  ) async => syncable = settings;
}

void main() {
  group('SettingsDialog Draft Behavior Tests', () {
    testWidgets(
      'modifying settings without saving does not affect controller and discards changes',
      (WidgetTester tester) async {
        final mockService = MockSettingsService();
        final controller = SettingsController(settingsService: mockService);

        await controller.init();

        await tester.pumpWidget(
          MaterialApp(
            home: Scaffold(body: SettingsDialog(controller: controller)),
          ),
        );

        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Find the '启用自动保存' switch
        final switchFinder = find.byType(Switch).first;
        expect(tester.widget<Switch>(switchFinder).value, true);

        // Toggle it to false
        await tester.tap(switchFinder);
        await tester.pump();

        // The UI should reflect the draft change
        expect(tester.widget<Switch>(switchFinder).value, false);

        // The controller should NOT have been updated yet
        expect(controller.syncableSettings.autoSaveEnabled, true);
      },
    );

    testWidgets('saving applies draft to controller', (
      WidgetTester tester,
    ) async {
      final mockService = MockSettingsService();
      final controller = SettingsController(settingsService: mockService);

      await controller.init();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: SettingsDialog(controller: controller)),
        ),
      );

      await tester.pump();

      // Find the switch
      final switchFinder = find.byType(Switch).first;

      // Toggle it to false
      await tester.tap(switchFinder);
      await tester.pump();

      // Click save button
      final saveButton = find.text('保存');
      await tester.tap(saveButton);
      await tester.pump();

      // We must wait for Future.microtask to complete the save()
      await tester.pump(const Duration(seconds: 1));

      // The controller should now be updated
      expect(controller.syncableSettings.autoSaveEnabled, false);
    });
  });
}
