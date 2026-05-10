import 'dart:io';
import 'package:flutter/material.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

import 'application/controllers/settings_controller.dart';
import 'presentation/screens/workspace_screen.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  if (Platform.isWindows || Platform.isLinux) {
    sqfliteFfiInit();
    databaseFactory = databaseFactoryFfi;
  }

  final settingsController = SettingsController();
  await settingsController.init();

  runApp(WriterApp(settingsController: settingsController));
}

class WriterApp extends StatelessWidget {
  final SettingsController settingsController;

  const WriterApp({super.key, required this.settingsController});

  ThemeMode _getThemeMode() {
    final mode = settingsController.syncableSettings.themeMode;
    switch (mode) {
      case 'light':
        return ThemeMode.light;
      case 'dark':
        return ThemeMode.dark;
      case 'system':
      default:
        return ThemeMode.system;
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settingsController,
      builder: (context, _) {
        return MaterialApp(
          debugShowCheckedModeBanner: false,
          title: 'Writer App',
          theme: ThemeData(
            colorScheme: ColorScheme.fromSeed(
              seedColor: Colors.deepPurple,
              brightness: Brightness.light,
            ),
            useMaterial3: true,
          ),
          darkTheme: ThemeData(
            colorScheme: ColorScheme.fromSeed(
              seedColor: Colors.deepPurple,
              brightness: Brightness.dark,
            ),
            useMaterial3: true,
          ),
          themeMode: _getThemeMode(),
          home: WorkspaceScreen(settingsController: settingsController),
        );
      },
    );
  }
}
