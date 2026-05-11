import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import '../../infrastructure/logging/app_logger.dart';

class SmoothScrollWrapper extends StatelessWidget {
  final Widget child;
  final ScrollController controller;
  final bool smoothScrollingEnabled;
  final int smoothScrollDurationMs;

  const SmoothScrollWrapper({
    super.key,
    required this.child,
    required this.controller,
    this.smoothScrollingEnabled = true,
    this.smoothScrollDurationMs = 120,
  });

  @override
  Widget build(BuildContext context) {
    if (!smoothScrollingEnabled) {
      return child;
    }

    return Listener(
      onPointerSignal: (pointerSignal) {
        if (pointerSignal is PointerScrollEvent) {
          _handleScroll(pointerSignal.scrollDelta.dy);
        }
      },
      child: child,
    );
  }

  void _handleScroll(double scrollDelta) {
    if (!controller.hasClients) {
      AppLogger.info(
        'Smooth scroll fallback: no clients attached',
        key: 'smooth_scroll_fallback',
        limitMs: 1000,
      );
      return;
    }

    try {
      final currentOffset = controller.offset;
      final targetOffset = currentOffset + scrollDelta;

      // Clamp target offset
      final maxScrollExtent = controller.position.maxScrollExtent;
      final minScrollExtent = controller.position.minScrollExtent;
      final clampedTargetOffset = targetOffset.clamp(
        minScrollExtent,
        maxScrollExtent,
      );

      controller.animateTo(
        clampedTargetOffset,
        duration: Duration(milliseconds: smoothScrollDurationMs),
        curve: Curves.easeOut,
      );
    } catch (e) {
      AppLogger.info(
        'Smooth scroll fallback: animateTo failed',
        key: 'smooth_scroll_fallback',
        limitMs: 1000,
      );
      // Fallback to jumpTo if animateTo fails (e.g. during composition/layout)
      try {
        controller.jumpTo(controller.offset + scrollDelta);
      } catch (_) {}
    }
  }
}
