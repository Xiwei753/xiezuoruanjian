import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import '../../infrastructure/logging/app_logger.dart';

class EditorInputAnimationOverlay extends StatefulWidget {
  final Widget child;
  final TextEditingController controller;
  final bool inputAnimationEnabled;
  final bool typedCharacterAnimationEnabled;
  final bool cursorAnimationEnhanced;
  final double editorFontSize;
  final String? activeChapterId; // Used to detect programmatic resets

  const EditorInputAnimationOverlay({
    super.key,
    required this.child,
    required this.controller,
    this.inputAnimationEnabled = false,
    this.typedCharacterAnimationEnabled = false,
    this.cursorAnimationEnhanced = false,
    this.editorFontSize = 16.0,
    this.activeChapterId,
  });

  @override
  State<EditorInputAnimationOverlay> createState() =>
      _EditorInputAnimationOverlayState();
}

class _EditorInputAnimationOverlayState
    extends State<EditorInputAnimationOverlay> {
  TextEditingValue _lastValue = TextEditingValue.empty;
  TextEditingValue _lastCommittedValue = TextEditingValue.empty;
  final List<_AnimationParticle> _particles = [];
  int _particleIdCounter = 0;
  Offset? _lastCaretOffset;

  @override
  void initState() {
    super.initState();
    _lastValue = widget.controller.value;
    _lastCommittedValue = widget.controller.value;
    widget.controller.addListener(_onTextChanged);
  }

  @override
  void didUpdateWidget(EditorInputAnimationOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onTextChanged);
      widget.controller.addListener(_onTextChanged);
      _lastValue = widget.controller.value;
      _lastCommittedValue = widget.controller.value;
    }
    // Suppress animation on chapter switch
    if (oldWidget.activeChapterId != widget.activeChapterId) {
      _lastValue = widget.controller.value;
      _lastCommittedValue = widget.controller.value;
    }
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onTextChanged);
    super.dispose();
  }

  void _onTextChanged() {
    if (!widget.inputAnimationEnabled) {
      _lastValue = widget.controller.value;
      _lastCommittedValue = widget.controller.value;
      return;
    }

    final newValue = widget.controller.value;

    // Strict skip for composing states
    if (newValue.composing.isValid && !newValue.composing.isCollapsed) {
      _lastValue = newValue;
      return;
    }

    // Suppress animation if the text was completely replaced programmatically (like load chapter)
    // We can infer this if the length difference is huge, but we already handle bulk paste skipping below.
    // However, if we just switched chapters, didUpdateWidget already updated _lastCommittedValue.

    final textChanged = _lastCommittedValue.text != newValue.text;
    final selectionChanged = _lastValue.selection != newValue.selection;

    if (textChanged && widget.typedCharacterAnimationEnabled) {
      final oldLen = _lastCommittedValue.text.length;
      final newLen = newValue.text.length;

      if (newLen > oldLen) {
        final insertedLen = newLen - oldLen;
        if (newValue.selection.isCollapsed &&
            newValue.selection.baseOffset >= insertedLen) {
          if (insertedLen == 1) {
            // Single character insertion: show the character
            final insertedChar = newValue.text.substring(
              newValue.selection.baseOffset - 1,
              newValue.selection.baseOffset,
            );
            _spawnParticle(insertedChar, isCursor: false);
          } else if (insertedLen <= 3) {
            // 2-3 characters insertion: show only the last character to avoid weird string animations
            final lastInsertedChar = newValue.text.substring(
              newValue.selection.baseOffset - 1,
              newValue.selection.baseOffset,
            );
            _spawnParticle(lastInsertedChar, isCursor: false);
          } else {
            // Bulk insertion (> 3 characters): just show a slight cursor pulse, do not log or show text
            _spawnParticle('', isCursor: true);
          }
        }
      }
    } else if (selectionChanged &&
        !textChanged &&
        widget.cursorAnimationEnhanced &&
        newValue.selection.isCollapsed) {
      // Just cursor moved
      final newOffset = _calculateCaretOffsetForCursor();
      if (_lastCaretOffset != null) {
        // Spawn trail
        _spawnCursorTrail(_lastCaretOffset!, newOffset);
      } else {
        // Just pulse
        _spawnParticle('', isCursor: true);
      }
      _lastCaretOffset = newOffset;
    }

    if (textChanged || (selectionChanged && newValue.selection.isCollapsed)) {
      // Keep track of the real offset when cursor moves or text changes
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          _lastCaretOffset = _getRawCaretRectOffset(true);
        }
      });
    }

    _lastValue = newValue;
    _lastCommittedValue = newValue;
  }

  RenderEditable? _findRenderEditable(RenderObject? root) {
    if (root == null || !root.attached) return null;
    if (root is RenderEditable) return root;

    RenderEditable? result;
    root.visitChildren((child) {
      result ??= _findRenderEditable(child);
    });
    return result;
  }

  Offset? _getRawCaretRectOffset(bool isCursor) {
    try {
      final RenderObject? renderObject = context.findRenderObject();
      if (renderObject == null ||
          renderObject is! RenderBox ||
          !renderObject.attached) {
        return null;
      }

      final RenderEditable? renderEditable = _findRenderEditable(renderObject);
      if (renderEditable == null || !renderEditable.attached) {
        return null;
      }

      final selection = widget.controller.selection;
      if (!selection.isValid) {
        return null;
      }

      final textLen = widget.controller.text.length;
      final clampedOffset = selection.baseOffset.clamp(0, textLen);

      // Calculate relative to RenderEditable
      final caretRect = renderEditable.getLocalRectForCaret(
        TextPosition(offset: clampedOffset),
      );

      Offset targetOffset;
      if (isCursor) {
        targetOffset = caretRect.topLeft;
      } else {
        // Character typed
        targetOffset = Offset(
          caretRect.left,
          caretRect.top - widget.editorFontSize * 0.05,
        );
      }

      // Convert from RenderEditable coordinates to Screen coordinates safely
      final globalOffset = renderEditable.localToGlobal(targetOffset);

      // Convert Screen coordinates to Overlay (our Stack) coordinates
      final localOffset = renderObject.globalToLocal(globalOffset);

      return localOffset;
    } catch (e) {
      // If layout fails or something is unattached, safely fallback.
      AppLogger.info(
        'Input animation caret offset fallback',
        key: 'caret_offset_fallback',
        limitMs: 1000,
      );
      return null;
    }
  }

  Offset _calculateCaretOffsetForCursor() {
    return _getRawCaretRectOffset(true) ?? const Offset(50.0, 50.0);
  }

  Offset _calculateCaretOffsetForTypedText() {
    return _getRawCaretRectOffset(false) ?? const Offset(50.0, 50.0);
  }

  void _spawnCursorTrail(Offset startOffset, Offset endOffset) {
    if (_particles.length >= 8) {
      _particles.removeAt(0);
    }

    final id = _particleIdCounter++;

    setState(() {
      _particles.add(
        _AnimationParticle(
          id: id,
          text: '',
          isCursor: true, // We reuse isCursor logic to style it
          offsetX: endOffset.dx,
          offsetY: endOffset.dy,
          startOffsetX: startOffset.dx,
          startOffsetY: startOffset.dy,
          isTrail: true,
        ),
      );
    });

    AppLogger.info(
      'Cursor trail spawned',
      key: 'cursor_trail_spawn',
      limitMs: 1000,
    );

    Future.delayed(const Duration(milliseconds: 240), () {
      if (mounted) {
        setState(() {
          _particles.removeWhere((p) => p.id == id);
        });
      }
    });
  }

  void _spawnParticle(String text, {required bool isCursor}) {
    if (_particles.length >= 8) {
      // Safety limit
      _particles.removeAt(0);
    }

    final id = _particleIdCounter++;
    final offset = isCursor
        ? _calculateCaretOffsetForCursor()
        : _calculateCaretOffsetForTypedText();

    setState(() {
      _particles.add(
        _AnimationParticle(
          id: id,
          text: text,
          isCursor: isCursor,
          offsetX: offset.dx,
          offsetY: offset.dy,
        ),
      );
    });

    if (isCursor) {
      AppLogger.info(
        'Input animation spawned',
        key: 'cursor_pulse_spawn',
        limitMs: 1000,
      );
    } else {
      AppLogger.info(
        'Input animation spawned',
        key: 'input_anim_spawn',
        limitMs: 1000,
      );
    }

    // Auto remove after animation
    Future.delayed(const Duration(milliseconds: 220), () {
      if (mounted) {
        setState(() {
          _particles.removeWhere((p) => p.id == id);
        });
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.inputAnimationEnabled) {
      return widget.child;
    }

    final textColor =
        Theme.of(context).textTheme.bodyMedium?.color ?? Colors.black;

    return Stack(
      children: [
        widget.child,
        // The overlay layer
        Positioned.fill(
          child: IgnorePointer(
            child: Stack(
              children: _particles
                  .map((p) => _buildParticleWidget(p, textColor))
                  .toList(),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildParticleWidget(_AnimationParticle particle, Color textColor) {
    // If it's a trail, we want to animate position, not just stay at particle.offsetX/Y
    if (particle.isTrail &&
        particle.startOffsetX != null &&
        particle.startOffsetY != null) {
      return TweenAnimationBuilder<double>(
        key: ValueKey(particle.id),
        tween: Tween(begin: 0.0, end: 1.0),
        duration: const Duration(milliseconds: 240),
        curve: Curves.easeOutCubic, // smoother deceleration
        builder: (context, value, child) {
          final currentX =
              particle.startOffsetX! +
              (particle.offsetX - particle.startOffsetX!) * value;
          final currentY =
              particle.startOffsetY! +
              (particle.offsetY - particle.startOffsetY!) * value;

          final opacity = (0.4 * (1.0 - value)).clamp(0.0, 1.0);

          return Positioned(
            left: currentX,
            top: currentY,
            child: Opacity(
              opacity: opacity,
              child: Container(
                width: 2,
                height: widget.editorFontSize * 1.6,
                color: textColor.withAlpha(150),
              ),
            ),
          );
        },
      );
    }

    return Positioned(
      left: particle.offsetX,
      top: particle.offsetY,
      child: TweenAnimationBuilder<double>(
        key: ValueKey(particle.id),
        tween: Tween(begin: 0.0, end: 1.0),
        duration: const Duration(milliseconds: 220),
        curve: Curves.easeOut,
        builder: (context, value, child) {
          if (particle.isCursor) {
            // Cursor pulse
            final opacity = (0.45 * (1.0 - value)).clamp(0.0, 1.0);
            return Opacity(
              opacity: opacity,
              child: Container(
                width: 2,
                height:
                    widget.editorFontSize *
                    1.6, // editorFontSize * editorLineHeight approx
                color: textColor.withAlpha(150),
              ),
            );
          } else {
            // Typed character scale up and fade out (in place)
            // Fade out is slower at the beginning
            final opacity = (1.0 - (value * value)).clamp(0.0, 1.0);
            return Opacity(
              opacity: opacity,
              child: Transform.scale(
                scale: 0.98 + (0.02 * value), // Very slight scale up
                child: Text(
                  particle.text,
                  style: TextStyle(
                    fontSize: widget.editorFontSize,
                    color: textColor,
                    fontWeight: FontWeight.normal,
                  ),
                ),
              ),
            );
          }
        },
      ),
    );
  }
}

class _AnimationParticle {
  final int id;
  final String text;
  final bool isCursor;
  final double offsetX;
  final double offsetY;
  final double? startOffsetX;
  final double? startOffsetY;
  final bool isTrail;

  _AnimationParticle({
    required this.id,
    required this.text,
    required this.isCursor,
    required this.offsetX,
    required this.offsetY,
    this.startOffsetX,
    this.startOffsetY,
    this.isTrail = false,
  });
}
