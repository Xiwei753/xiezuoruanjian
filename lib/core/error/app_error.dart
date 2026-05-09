enum ErrorCode {
  unknown,
  saveFailure,
  fileSystemError,
  cacheRebuildError,
}

class AppError implements Exception {
  final ErrorCode code;
  final String userMessage;
  final String debugMessage;
  final dynamic originalError;

  AppError({
    required this.code,
    required this.userMessage,
    required this.debugMessage,
    this.originalError,
  });

  @override
  String toString() {
    return 'AppError[$code]: $debugMessage (User: $userMessage)\nOriginal: $originalError';
  }
}
