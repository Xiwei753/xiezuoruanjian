import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

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

      // Allow animating a few characters for Chinese commit
      if (newLen > oldLen && (newLen - oldLen) <= 3) {
        if (newValue.selection.isCollapsed &&
            newValue.selection.baseOffset >= (newLen - oldLen)) {
          final insertedChar = newValue.text.substring(
            newValue.selection.baseOffset - (newLen - oldLen),
            newValue.selection.baseOffset,
          );

          _spawnParticle(insertedChar, isCursor: false);
        }
      }
    } else if (selectionChanged &&
        !textChanged &&
        widget.cursorAnimationEnhanced &&
        newValue.selection.isCollapsed) {
      // Just cursor moved
      _spawnParticle('', isCursor: true);
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

  Offset _calculateCaretOffset() {
    // Fallback default coordinates
    const fallbackOffset = Offset(50.0, 50.0);

    try {
      final RenderObject? renderObject = context.findRenderObject();
      if (renderObject == null ||
          renderObject is! RenderBox ||
          !renderObject.attached) {
        return fallbackOffset;
      }

      final RenderEditable? renderEditable = _findRenderEditable(renderObject);
      if (renderEditable == null || !renderEditable.attached) {
        return fallbackOffset;
      }

      final selection = widget.controller.selection;
      if (!selection.isValid) {
        return fallbackOffset;
      }

      final textLen = widget.controller.text.length;
      final clampedOffset = selection.baseOffset.clamp(0, textLen);

      // Calculate relative to RenderEditable
      final caretRect = renderEditable.getLocalRectForCaret(
        TextPosition(offset: clampedOffset),
      );

      // Convert from RenderEditable coordinates to Screen coordinates safely
      final globalOffset = renderEditable.localToGlobal(caretRect.bottomLeft);

      // Convert Screen coordinates to Overlay (our Stack) coordinates
      final localOffset = renderObject.globalToLocal(globalOffset);

      return localOffset;
    } catch (e) {
      // If layout fails or something is unattached, safely fallback.
      return fallbackOffset;
    }
  }

  void _spawnParticle(String text, {required bool isCursor}) {
    if (_particles.length >= 8) {
      // Safety limit
      _particles.removeAt(0);
    }

    final id = _particleIdCounter++;
    final offset = _calculateCaretOffset();

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

    // Auto remove after animation
    Future.delayed(const Duration(milliseconds: 150), () {
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
    return Positioned(
      left: particle.offsetX,
      top: particle.offsetY,
      child: TweenAnimationBuilder<double>(
        key: ValueKey(particle.id),
        tween: Tween(begin: 0.0, end: 1.0),
        duration: const Duration(milliseconds: 150),
        curve: Curves.easeOut,
        builder: (context, value, child) {
          if (particle.isCursor) {
            // Cursor pulse
            return Opacity(
              opacity: 0.5 * (1.0 - value),
              child: Container(
                width: 2,
                height: widget.editorFontSize * 1.2,
                color: textColor.withAlpha(100),
              ),
            );
          } else {
            // Typed character scale up and fade out
            return Opacity(
              opacity: 1.0 - value,
              child: Transform.scale(
                scale: 0.96 + (0.04 * value),
                child: Transform.translate(
                  offset: Offset(0, -2 * value), // Very slight upward movement
                  child: Text(
                    particle.text,
                    style: TextStyle(
                      fontSize: widget.editorFontSize,
                      color: textColor,
                      fontWeight: FontWeight.normal,
                    ),
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

  _AnimationParticle({
    required this.id,
    required this.text,
    required this.isCursor,
    required this.offsetX,
    required this.offsetY,
  });
}
