import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/presentation/widgets/editor_input_animation_overlay.dart';

void main() {
  group('EditorInputAnimationOverlay Tests', () {
    testWidgets('Disabled settings do not render animation layer', (
      WidgetTester tester,
    ) async {
      final controller = TextEditingController();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: EditorInputAnimationOverlay(
              controller: controller,
              inputAnimationEnabled: false,
              child: TextField(controller: controller),
            ),
          ),
        ),
      );

      // Verify the child TextField is present but no Stack is created by overlay
      expect(find.byType(TextField), findsOneWidget);

      // When not enabled, EditorInputAnimationOverlay just returns widget.child (TextField) directly, not wrapped in Stack.
      // We can verify this by checking that there are no Positioned widgets
      expect(find.byType(Positioned), findsNothing);
    });

    testWidgets('Composing state skips character animation', (
      WidgetTester tester,
    ) async {
      final controller = TextEditingController();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: EditorInputAnimationOverlay(
              controller: controller,
              inputAnimationEnabled: true,
              typedCharacterAnimationEnabled: true,
              child: TextField(controller: controller),
            ),
          ),
        ),
      );

      expect(
        find.byType(Positioned),
        findsWidgets,
      ); // Contains Positioned.fill wrapper

      // Update text with composing valid
      controller.value = const TextEditingValue(
        text: 'abc',
        composing: TextRange(start: 0, end: 3),
      );

      await tester.pump();
      // Should not spawn any animation particles
      expect(find.byType(TweenAnimationBuilder<double>), findsNothing);
    });

    testWidgets('Single character input creates animation particle', (
      WidgetTester tester,
    ) async {
      final controller = TextEditingController();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: EditorInputAnimationOverlay(
              controller: controller,
              inputAnimationEnabled: true,
              typedCharacterAnimationEnabled: true,
              child: TextField(controller: controller),
            ),
          ),
        ),
      );

      // Simulate inserting 'A'
      controller.value = const TextEditingValue(
        text: 'A',
        selection: TextSelection.collapsed(offset: 1),
      );

      await tester.pump();

      // Should spawn one TweenAnimationBuilder
      expect(find.byType(TweenAnimationBuilder<double>), findsOneWidget);
      expect(
        find.text('A'),
        findsWidgets,
      ); // Will find 2: one in TextField, one in Overlay

      // Fast forward time, particle should disappear
      await tester.pump(const Duration(milliseconds: 300));
      await tester.pump();

      expect(find.byType(TweenAnimationBuilder<double>), findsNothing);
    });

    testWidgets('Large paste does not trigger animation', (
      WidgetTester tester,
    ) async {
      final controller = TextEditingController();

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: EditorInputAnimationOverlay(
              controller: controller,
              inputAnimationEnabled: true,
              typedCharacterAnimationEnabled: true,
              child: TextField(controller: controller),
            ),
          ),
        ),
      );

      // Simulate pasting huge chunk
      controller.value = const TextEditingValue(
        text: 'This is a large block of text being pasted all at once',
        selection: TextSelection.collapsed(offset: 54),
      );

      await tester.pump();

      // Should NOT spawn animation for bulk text
      expect(find.byType(TweenAnimationBuilder<double>), findsNothing);
    });

    testWidgets('Cursor movement creates cursor pulse particle', (
      WidgetTester tester,
    ) async {
      final controller = TextEditingController(text: 'Hello World');

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: EditorInputAnimationOverlay(
              controller: controller,
              inputAnimationEnabled: true,
              cursorAnimationEnhanced: true,
              typedCharacterAnimationEnabled: false,
              child: TextField(controller: controller),
            ),
          ),
        ),
      );

      // Move cursor
      controller.value = const TextEditingValue(
        text: 'Hello World',
        selection: TextSelection.collapsed(offset: 5),
      );

      await tester.pump();

      // Should spawn one TweenAnimationBuilder (cursor pulse)
      expect(find.byType(TweenAnimationBuilder<double>), findsOneWidget);

      // Finish timer
      await tester.pump(const Duration(milliseconds: 300));
      await tester.pump();
    });
  });
}
