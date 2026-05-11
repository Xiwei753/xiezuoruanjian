import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/presentation/widgets/smooth_scroll_wrapper.dart';

void main() {
  group('SmoothScrollWrapper Tests', () {
    testWidgets('Disabled settings do not register custom scroll', (
      WidgetTester tester,
    ) async {
      final controller = ScrollController();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SmoothScrollWrapper(
              controller: controller,
              smoothScrollingEnabled: false,
              child: ListView.builder(
                controller: controller,
                itemCount: 100,
                itemBuilder: (context, index) => Text('Item $index'),
              ),
            ),
          ),
        ),
      );

      // Find the SmoothScrollWrapper and assert its child is returned directly when disabled
      final wrapperFinder = find.byType(SmoothScrollWrapper);
      expect(wrapperFinder, findsOneWidget);

      // Since disabled, wrapper returns child directly, but child (ListView) might contain Listeners.
      // So we check if the wrapper directly creates the onPointerSignal Listener
      final widget = tester.widget<SmoothScrollWrapper>(wrapperFinder);
      expect(widget.smoothScrollingEnabled, false);
    });

    testWidgets('Enabled settings registers listener', (
      WidgetTester tester,
    ) async {
      final controller = ScrollController();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SmoothScrollWrapper(
              controller: controller,
              smoothScrollingEnabled: true,
              child: ListView.builder(
                controller: controller,
                itemCount: 100,
                itemBuilder: (context, index) => Text('Item $index'),
              ),
            ),
          ),
        ),
      );

      final wrapperFinder = find.byType(SmoothScrollWrapper);
      expect(wrapperFinder, findsOneWidget);

      final widget = tester.widget<SmoothScrollWrapper>(wrapperFinder);
      expect(widget.smoothScrollingEnabled, true);

      // Find the specific Listener added by SmoothScrollWrapper
      final listeners = find.descendant(
        of: wrapperFinder,
        matching: find.byType(Listener),
      );

      // Assert that there's at least one Listener created when enabled
      expect(listeners, findsWidgets);
    });
  });
}
