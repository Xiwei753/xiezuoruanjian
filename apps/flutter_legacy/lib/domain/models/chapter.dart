class Chapter {
  final String id;
  final String volumeId;
  final String projectId;
  final String title;
  final DateTime createdAt;
  final DateTime updatedAt;
  final String contentHash;
  final int wordCount;

  const Chapter({
    required this.id,
    required this.volumeId,
    required this.projectId,
    required this.title,
    required this.createdAt,
    required this.updatedAt,
    required this.contentHash,
    required this.wordCount,
  });

  Chapter copyWith({
    String? id,
    String? volumeId,
    String? projectId,
    String? title,
    DateTime? createdAt,
    DateTime? updatedAt,
    String? contentHash,
    int? wordCount,
  }) {
    return Chapter(
      id: id ?? this.id,
      volumeId: volumeId ?? this.volumeId,
      projectId: projectId ?? this.projectId,
      title: title ?? this.title,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      contentHash: contentHash ?? this.contentHash,
      wordCount: wordCount ?? this.wordCount,
    );
  }
}
