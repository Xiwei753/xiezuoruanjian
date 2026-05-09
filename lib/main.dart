import 'dart:io';
import 'package:flutter/material.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

import 'presentation/screens/workspace_screen.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  if (Platform.isWindows || Platform.isLinux) {
    sqfliteFfiInit();
    databaseFactory = databaseFactoryFfi;
  }

  runApp(const WriterApp());
}

class WriterApp extends StatelessWidget {
  const WriterApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Writer App',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple, brightness: Brightness.dark),
        useMaterial3: true,
      ),
      home: const WorkspaceScreen(),
    );
  }
}
