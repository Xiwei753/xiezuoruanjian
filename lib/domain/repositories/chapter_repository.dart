import '../models/chapter.dart';

abstract class IChapterRepository {
  Future<void> saveChapter(Chapter chapter, String content);
  Future<String> readChapterContent(Chapter chapter);
  Future<void> rebuildIndexFromWorkspace(String projectId);
}
