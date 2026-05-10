import 'package:flutter/foundation.dart';

import '../../domain/models/settings.dart';
import '../../domain/services_interfaces/settings_service.dart';
import '../../infrastructure/settings/settings_service_impl.dart';

class SettingsController extends ChangeNotifier {
  final ISettingsService _settingsService;

  SettingsController({ISettingsService? settingsService})
    : _settingsService = settingsService ?? SettingsServiceImpl();

  LocalSettings _localSettings = const LocalSettings();
  SyncableSettings _syncableSettings = const SyncableSettings();

  bool _isLoading = true;
  bool _isSaving = false;
  String? _errorMessage;
  bool _isDirty = false;

  LocalSettings get localSettings => _localSettings;
  SyncableSettings get syncableSettings => _syncableSettings;

  bool get isLoading => _isLoading;
  bool get isSaving => _isSaving;
  String? get errorMessage => _errorMessage;
  bool get isDirty => _isDirty;

  String get workspacePath => _localSettings.workspacePath;

  Future<void> init() async {
    _isLoading = true;
    _errorMessage = null;
    notifyListeners();

    try {
      _localSettings = await _settingsService.loadLocalSettings();
      // If workspace path is empty in local settings, we might need a default or let the app provide it.
      // But the requirement says WorkspaceController sets it.
      // For now, if it's empty, we just load SyncableSettings with an empty path which will fail or save locally,
      // but in real app, WorkspaceController sets workspacePath in LocalSettings eventually.
      // Wait, we need the workspacePath to load syncable settings.
      if (_localSettings.workspacePath.isNotEmpty) {
        _syncableSettings = await _settingsService.loadSyncableSettings(
          _localSettings.workspacePath,
        );
      }
    } catch (e) {
      _errorMessage = 'Failed to load settings: $e';
    } finally {
      _isLoading = false;
      _isDirty = false;
      notifyListeners();
    }
  }

  // Load with a specific workspace path (useful when WorkspaceController initializes it)
  Future<void> initWithWorkspacePath(String path) async {
    _isLoading = true;
    _errorMessage = null;
    notifyListeners();

    try {
      _localSettings = await _settingsService.loadLocalSettings();
      if (_localSettings.workspacePath != path) {
        _localSettings = _localSettings.copyWith(workspacePath: path);
        await _settingsService.saveLocalSettings(_localSettings);
      }
      _syncableSettings = await _settingsService.loadSyncableSettings(path);
    } catch (e) {
      _errorMessage = 'Failed to load settings: $e';
    } finally {
      _isLoading = false;
      _isDirty = false;
      notifyListeners();
    }
  }

  void updateLocalSettings(LocalSettings newSettings) {
    _localSettings = newSettings;
    _isDirty = true;
    notifyListeners();
  }

  void updateSyncableSettings(SyncableSettings newSettings) {
    _syncableSettings = newSettings;
    _isDirty = true;
    notifyListeners();
  }

  Future<void> saveLocal() async {
    try {
      await _settingsService.saveLocalSettings(_localSettings);
    } catch (e) {
      _errorMessage = 'Failed to save local settings: $e';
      notifyListeners();
    }
  }

  Future<void> save() async {
    if (!_isDirty) return;

    _isSaving = true;
    _errorMessage = null;
    notifyListeners();

    try {
      await _settingsService.saveLocalSettings(_localSettings);
      if (_localSettings.workspacePath.isNotEmpty) {
        await _settingsService.saveSyncableSettings(
          _localSettings.workspacePath,
          _syncableSettings,
        );
      }
      _isDirty = false;
    } catch (e) {
      _errorMessage = 'Failed to save settings: $e';
    } finally {
      _isSaving = false;
      notifyListeners();
    }
  }
}
