class CorrectionIssue {
  final int startOffset;
  final int endOffset;
  final String suggestion;
  CorrectionIssue({required this.startOffset, required this.endOffset, required this.suggestion});
}

abstract class ICorrectionEngine {
  Future<List<CorrectionIssue>> scanText(String text, String chapterId);
}
