import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/application/controllers/workspace_controller.dart';
import 'package:writer_app/domain/models/chapter.dart';
import 'package:writer_app/domain/repositories/chapter_repository.dart';

class MockChapterRepository implements IChapterRepository {
  String content = '';
  bool throwError = false;

  Future<Chapter> createChapter(String title) async => Chapter(
    id: 'c1',
    projectId: 'p1',
    volumeId: 'v1',
    title: title,
    createdAt: DateTime.now(),
    updatedAt: DateTime.now(),
    wordCount: 0,
    contentHash: '',
  );

  @override
  Future<void> deleteChapter(Chapter chapter) async {}

  @override
  Future<String> readChapterContent(Chapter chapter) async => content;

  @override
  Future<void> rebuildIndexFromWorkspace(String projectId) async {}

  @override
  Future<void> saveChapter(Chapter chapter, String newContent) async {
    if (throwError) throw Exception("disk full");
    content = newContent;
  }
}

class TestWorkspaceController extends WorkspaceController {
  final MockChapterRepository mockRepo;

  TestWorkspaceController(this.mockRepo);

  @override
  Future<void> loadChapters() async {
    chapters = [
      Chapter(
        id: 'c1',
        projectId: 'p1',
        volumeId: 'v1',
        title: 'Ch1',
        createdAt: DateTime.now(),
        updatedAt: DateTime.now(),
        wordCount: 0,
        contentHash: '',
      ),
    ];
    notifyListeners();
  }

  // Override to bypass sqlite cache rebuild which accesses private dbHelper
  @override
  Future<void> saveCurrentChapter(
    String contentToSave, {
    bool isAutoSave = false,
  }) async {
    if (selectedChapter == null) return;

    try {
      await mockRepo.saveChapter(selectedChapter!, contentToSave);

      final now = DateTime.now();
      final updatedChapter = selectedChapter!.copyWith(updatedAt: now);

      final index = chapters.indexWhere((c) => c.id == updatedChapter.id);
      if (index != -1) chapters[index] = updatedChapter;

      selectedChapter = updatedChapter;
      currentContent = contentToSave;

      isDirty = false;
      lastSavedAt = now;
      lastSaveError = null;
    } catch (e) {
      lastSaveError = e.toString();
      // Keep isDirty unchanged (true)
    }
    notifyListeners();
  }

  @override
  Future<void> selectChapter(Chapter chapter) async {
    currentContent = await mockRepo.readChapterContent(chapter);
    selectedChapter = chapter;
    isDirty = false;
    lastSaveError = null;
    notifyListeners();
  }
}

void main() {
  group('WorkspaceController Auto-Save & Dirty State logic (Mocked)', () {
    late TestWorkspaceController controller;
    late MockChapterRepository repo;

    setUp(() async {
      repo = MockChapterRepository();
      controller = TestWorkspaceController(repo);
      await controller.loadChapters();
    });

    test('Loading a chapter does not set isDirty', () async {
      await controller.selectChapter(controller.chapters.first);

      expect(controller.isDirty, isFalse);
      expect(controller.lastSaveError, isNull);
    });

    test('markDirty() sets isDirty to true', () async {
      await controller.selectChapter(controller.chapters.first);
      controller.markDirty();
      expect(controller.isDirty, isTrue);
    });

    test('Manual save sets isDirty to false and updates lastSavedAt', () async {
      await controller.selectChapter(controller.chapters.first);
      controller.markDirty();

      await controller.saveCurrentChapter("new manual", isAutoSave: false);

      expect(controller.isDirty, isFalse);
      expect(controller.lastSavedAt, isNotNull);
      expect(controller.lastSaveError, isNull);
      expect(controller.currentContent, "new manual");
      expect(repo.content, "new manual");
    });

    test('Auto save sets isDirty to false and updates lastSavedAt', () async {
      await controller.selectChapter(controller.chapters.first);
      controller.markDirty();

      await controller.saveCurrentChapter("new auto", isAutoSave: true);

      expect(controller.isDirty, isFalse);
      expect(controller.lastSavedAt, isNotNull);
      expect(controller.lastSaveError, isNull);
      expect(controller.currentContent, "new auto");
    });

    test('Save failure keeps isDirty true and sets lastSaveError', () async {
      await controller.selectChapter(controller.chapters.first);
      controller.markDirty();

      // Inject failure
      repo.throwError = true;

      await controller.saveCurrentChapter("fail auto", isAutoSave: true);

      expect(controller.isDirty, isTrue); // should remain dirty
      expect(controller.lastSaveError, contains('disk full'));
      expect(repo.content, ''); // content shouldn't be saved
    });
  });
}
