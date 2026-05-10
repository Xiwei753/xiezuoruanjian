import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:path/path.dart' as p;
import 'package:writer_app/domain/models/settings.dart';
import 'package:writer_app/domain/services_interfaces/storage_service.dart';
import 'package:writer_app/infrastructure/settings/settings_service_impl.dart';
import 'package:flutter/services.dart';

class MockStorageService implements IStorageService {
  String? lastWrittenPath;
  String? lastWrittenContent;

  @override
  Future<void> atomicWrite(String filePath, String content) async {
    lastWrittenPath = filePath;
    lastWrittenContent = content;

    final file = File(filePath);
    final parent = file.parent;
    if (!await parent.exists()) {
      await parent.create(recursive: true);
    }
    await file.writeAsString(content);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late MockStorageService mockStorageService;
  late SettingsServiceImpl settingsService;
  late Directory tempDir;
  late String workspacePath;

  setUp(() async {
    tempDir = await Directory.systemTemp.createTemp('settings_service_test');
    workspacePath = p.join(tempDir.path, 'workspace');

    // Mock path_provider MethodChannel
    const MethodChannel channel = MethodChannel(
      'plugins.flutter.io/path_provider',
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (MethodCall methodCall) async {
          if (methodCall.method == 'getApplicationSupportDirectory') {
            return tempDir.path;
          }
          return null;
        });

    mockStorageService = MockStorageService();
    settingsService = SettingsServiceImpl(storageService: mockStorageService);
  });

  tearDown(() async {
    if (await tempDir.exists()) {
      await tempDir.delete(recursive: true);
    }
  });

  group('SettingsService', () {
    test(
      'automatically creates default configuration when settings.sync.json does not exist',
      () async {
        final syncFile = File(
          p.join(workspacePath, 'app-meta', 'settings', 'settings.sync.json'),
        );
        expect(await syncFile.exists(), false);

        final settings = await settingsService.loadSyncableSettings(
          workspacePath,
        );

        expect(settings.schemaVersion, 1);
        expect(mockStorageService.lastWrittenPath, syncFile.path);
        expect(await syncFile.exists(), true);
      },
    );

    test(
      'does not crash the App when JSON is corrupted (SyncableSettings)',
      () async {
        final syncFile = File(
          p.join(workspacePath, 'app-meta', 'settings', 'settings.sync.json'),
        );
        await syncFile.parent.create(recursive: true);
        await syncFile.writeAsString('{"invalid_json": ');

        final settings = await settingsService.loadSyncableSettings(
          workspacePath,
        );

        expect(settings.schemaVersion, 1); // Returns default
      },
    );

    test(
      'does not crash the App when JSON is corrupted (LocalSettings)',
      () async {
        final localFile = File(p.join(tempDir.path, 'settings.local.json'));
        await localFile.writeAsString('{"invalid_json": ');

        final settings = await settingsService.loadLocalSettings();

        expect(settings.workspacePath, ''); // Returns default
      },
    );

    test('saving settings must go through AtomicWriter', () async {
      const syncable = SyncableSettings(editorFontSize: 24.0);
      await settingsService.saveSyncableSettings(workspacePath, syncable);

      expect(
        mockStorageService.lastWrittenPath,
        p.join(workspacePath, 'app-meta', 'settings', 'settings.sync.json'),
      );
      expect(
        mockStorageService.lastWrittenContent,
        contains('"editorFontSize": 24.0'),
      );
    });
  });
}
