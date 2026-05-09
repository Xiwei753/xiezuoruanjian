import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/domain/models/project.dart';
import 'package:writer_app/domain/models/chapter.dart';

void main() {
  group('Domain Models Tests', () {
    test('Project copyWith should update fields correctly', () {
      final now = DateTime.now();
      final project = Project(id: '1', title: 'My Novel', createdAt: now, updatedAt: now);

      final updatedProject = project.copyWith(title: 'Updated Novel');

      expect(updatedProject.id, '1');
      expect(updatedProject.title, 'Updated Novel');
      expect(updatedProject.createdAt, now);
      expect(updatedProject.updatedAt, now);
    });

    test('Chapter copyWith should update fields correctly', () {
      final now = DateTime.now();
      final chapter = Chapter(
        id: 'c1',
        volumeId: 'v1',
        projectId: 'p1',
        title: 'Chapter 1',
        createdAt: now,
        updatedAt: now,
        contentHash: 'hash',
        wordCount: 100,
      );

      final updatedChapter = chapter.copyWith(wordCount: 200, contentHash: 'newHash');

      expect(updatedChapter.id, 'c1');
      expect(updatedChapter.title, 'Chapter 1');
      expect(updatedChapter.wordCount, 200);
      expect(updatedChapter.contentHash, 'newHash');
    });
  });
}
