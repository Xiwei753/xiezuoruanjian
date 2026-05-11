class WorkspaceManifest {
  final int schemaVersion;
  final String id;
  final DateTime createdAt;

  WorkspaceManifest({
    this.schemaVersion = 1,
    required this.id,
    required this.createdAt,
  });

  Map<String, dynamic> toJson() => {
    'schemaVersion': schemaVersion,
    'id': id,
    'createdAt': createdAt.toIso8601String(),
  };

  factory WorkspaceManifest.fromJson(Map<String, dynamic> json) =>
      WorkspaceManifest(
        schemaVersion: json['schemaVersion'] as int? ?? 1,
        id: json['id'] as String,
        createdAt: DateTime.parse(json['createdAt'] as String),
      );
}

class ProjectManifest {
  final String id;
  final String title;
  final DateTime createdAt;

  ProjectManifest({
    required this.id,
    required this.title,
    required this.createdAt,
  });

  Map<String, dynamic> toJson() => {
    'id': id,
    'title': title,
    'createdAt': createdAt.toIso8601String(),
  };

  factory ProjectManifest.fromJson(Map<String, dynamic> json) =>
      ProjectManifest(
        id: json['id'] as String,
        title: json['title'] as String,
        createdAt: DateTime.parse(json['createdAt'] as String),
      );
}

class VolumeMeta {
  final String id;
  final String title;
  final DateTime createdAt;

  VolumeMeta({required this.id, required this.title, required this.createdAt});

  Map<String, dynamic> toJson() => {
    'id': id,
    'title': title,
    'createdAt': createdAt.toIso8601String(),
  };

  factory VolumeMeta.fromJson(Map<String, dynamic> json) => VolumeMeta(
    id: json['id'] as String,
    title: json['title'] as String,
    createdAt: DateTime.parse(json['createdAt'] as String),
  );
}

class ChapterMeta {
  final String id;
  final String volumeId;
  final String projectId;
  final String title;
  final DateTime createdAt;
  final DateTime updatedAt;
  final String contentHash;
  final int wordCount;

  ChapterMeta({
    required this.id,
    required this.volumeId,
    required this.projectId,
    required this.title,
    required this.createdAt,
    required this.updatedAt,
    required this.contentHash,
    required this.wordCount,
  });

  Map<String, dynamic> toJson() => {
    'id': id,
    'volumeId': volumeId,
    'projectId': projectId,
    'title': title,
    'createdAt': createdAt.toIso8601String(),
    'updatedAt': updatedAt.toIso8601String(),
    'contentHash': contentHash,
    'wordCount': wordCount,
  };

  factory ChapterMeta.fromJson(Map<String, dynamic> json) => ChapterMeta(
    id: json['id'] as String,
    volumeId: json['volumeId'] as String,
    projectId: json['projectId'] as String,
    title: json['title'] as String,
    createdAt: DateTime.parse(json['createdAt'] as String),
    updatedAt: DateTime.parse(json['updatedAt'] as String),
    contentHash: json['contentHash'] as String,
    wordCount: json['wordCount'] as int,
  );
}
