import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import '../../infrastructure/logging/app_logger.dart';

class SmoothScrollWrapper extends StatefulWidget {
  final Widget child;
  final ScrollController controller;
  final bool smoothScrollingEnabled;
  final int smoothScrollDurationMs;

  const SmoothScrollWrapper({
    super.key,
    required this.child,
    required this.controller,
    this.smoothScrollingEnabled = true,
    this.smoothScrollDurationMs = 240,
  });

  @override
  State<SmoothScrollWrapper> createState() => _SmoothScrollWrapperState();
}

class _SmoothScrollWrapperState extends State<SmoothScrollWrapper> {
  double? _targetOffset;

  void _handlePointerSignal(PointerSignalEvent event) {
    if (event is PointerScrollEvent) {
      GestureBinding.instance.pointerSignalResolver.register(event, (
        PointerSignalEvent e,
      ) {
        if (e is PointerScrollEvent) {
          _handleScroll(e.scrollDelta.dy);
        }
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.smoothScrollingEnabled) {
      return widget.child;
    }

    return Listener(onPointerSignal: _handlePointerSignal, child: widget.child);
  }

  void _handleScroll(double scrollDelta) {
    if (!widget.controller.hasClients) {
      AppLogger.info(
        'Smooth scroll fallback: no clients attached',
        key: 'smooth_scroll_fallback',
        limitMs: 1000,
      );
      return;
    }

    try {
      final currentOffset = widget.controller.offset;

      // If we already have a target, add to it, otherwise add to current
      _targetOffset = (_targetOffset ?? currentOffset) + scrollDelta;

      // Clamp target offset
      final maxScrollExtent = widget.controller.position.maxScrollExtent;
      final minScrollExtent = widget.controller.position.minScrollExtent;
      final clampedTargetOffset = _targetOffset!.clamp(
        minScrollExtent,
        maxScrollExtent,
      );

      _targetOffset = clampedTargetOffset;

      widget.controller
          .animateTo(
            clampedTargetOffset,
            duration: Duration(milliseconds: widget.smoothScrollDurationMs),
            curve: Curves.easeOut,
          )
          .then((_) {
            // Only clear if we've reached our destination (not interrupted)
            if (mounted &&
                widget.controller.hasClients &&
                widget.controller.offset == _targetOffset) {
              _targetOffset = null;
            }
          });
    } catch (e) {
      AppLogger.info(
        'Smooth scroll fallback: animateTo failed',
        key: 'smooth_scroll_fallback',
        limitMs: 1000,
      );
      // Fallback to jumpTo if animateTo fails (e.g. during composition/layout)
      try {
        widget.controller.jumpTo(widget.controller.offset + scrollDelta);
      } catch (_) {}
      _targetOffset = null;
    }
  }
}
