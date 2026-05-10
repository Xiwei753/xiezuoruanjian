import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/infrastructure/logging/app_logger.dart';

void main() {
  group('AppLogger Tests', () {
    test(
      'sanitizeMetadata hides apiKey, token, password, secret and limits length',
      () {
        final input = {
          'deepSeekApiKey': 'sk-1234567890abcdef',
          'githubToken': 'ghp_xyz123',
          'userPassword': 'mysecretpassword',
          'clientSecret': 'super_secret',
          'normalField': 'hello',
          'longField': List.generate(300, (_) => 'A').join(),
          'nested': {'token': '123'},
          'content': 'This should be redacted',
        };

        final output = AppLogger.sanitizeMetadata(input);

        expect(output['deepSeekApiKey'], '***');
        expect(output['githubToken'], '***');
        expect(output['userPassword'], '***');
        expect(output['clientSecret'], '***');
        expect(output['normalField'], 'hello');
        expect(
          output['longField'].length,
          214,
        ); // 200 + 14 for '...[TRUNCATED]'
        expect(output['longField'].endsWith('...[TRUNCATED]'), true);
        expect((output['nested'] as Map)['token'], '***');
        expect(output['content'], '[REDACTED]');
      },
    );
  });

  test('AppLogger handles JSON output format properly without crashing', () {
    // This is more to ensure there are no simple runtime syntax errors in _log format.
    AppLogger.configure(
      loggingEnabled: true,
      debugLoggingEnabled: true,
      performanceLoggingEnabled: true,
      slowOperationThresholdMs: 10,
    );
    // Should not throw
    AppLogger.debug('test message', {'key': 'value'});
    AppLogger.info('info message');
    AppLogger.warning('warning message');
    AppLogger.error('error message', Exception('test'), StackTrace.empty);
    AppLogger.performance('perf_test', 50);
  });
}
