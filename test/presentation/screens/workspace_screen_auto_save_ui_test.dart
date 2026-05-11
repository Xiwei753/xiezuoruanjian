import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/application/controllers/workspace_controller.dart';
import 'package:writer_app/domain/models/chapter.dart';
import 'package:writer_app/presentation/widgets/save_status_indicator.dart';

// Since the whole WorkspaceScreen is hard to mock due to DB and filesystem dependencies,
// we will unit test the SaveStatusIndicator UI logic directly.
class MockUIWorkspaceController extends WorkspaceController {
  void triggerUpdate() {
    notifyListeners();
  }
}

void main() {
  group('SaveStatusIndicator UI Tests', () {
    late MockUIWorkspaceController controller;

    setUp(() {
      controller = MockUIWorkspaceController();
      controller.selectedChapter = Chapter(
        id: 'c1',
        projectId: 'p1',
        volumeId: 'v1',
        title: 'Ch1',
        createdAt: DateTime.now(),
        updatedAt: DateTime.now(),
        wordCount: 0,
        contentHash: '',
      );
    });

    testWidgets('Shows "保存中..." when isSaving is true', (
      WidgetTester tester,
    ) async {
      controller.isSaving = true;

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: SaveStatusIndicator(controller: controller)),
        ),
      );

      expect(find.text('保存中...'), findsOneWidget);
      expect(find.byType(CircularProgressIndicator), findsOneWidget);
    });

    testWidgets('Shows "保存失败" when lastSaveError is not null', (
      WidgetTester tester,
    ) async {
      controller.lastSaveError = 'disk full';

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: SaveStatusIndicator(controller: controller)),
        ),
      );

      expect(find.text('保存失败'), findsOneWidget);
      expect(find.byIcon(Icons.error_outline), findsOneWidget);
    });

    testWidgets('Shows "未保存" when isDirty is true', (
      WidgetTester tester,
    ) async {
      controller.isDirty = true;

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: SaveStatusIndicator(controller: controller)),
        ),
      );

      expect(find.text('未保存'), findsOneWidget);
      expect(find.byIcon(Icons.edit), findsOneWidget);
    });

    testWidgets('Shows "已保存 HH:mm:ss" when saved cleanly', (
      WidgetTester tester,
    ) async {
      controller.isDirty = false;
      controller.lastSavedAt = DateTime(2024, 1, 1, 14, 30, 5); // 14:30:05

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: SaveStatusIndicator(controller: controller)),
        ),
      );

      expect(find.text('已保存 14:30:05'), findsOneWidget);
      expect(find.byIcon(Icons.check_circle_outline), findsOneWidget);
    });
  });
}
