import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'dart:math';

class EditorInputAnimationOverlay extends StatefulWidget {
  final Widget child;
  final TextEditingController controller;
  final bool inputAnimationEnabled;
  final bool typedCharacterAnimationEnabled;
  final bool cursorAnimationEnhanced;

  const EditorInputAnimationOverlay({
    super.key,
    required this.child,
    required this.controller,
    this.inputAnimationEnabled = false,
    this.typedCharacterAnimationEnabled = false,
    this.cursorAnimationEnhanced = false,
  });

  @override
  State<EditorInputAnimationOverlay> createState() =>
      _EditorInputAnimationOverlayState();
}

class _EditorInputAnimationOverlayState
    extends State<EditorInputAnimationOverlay> {
  TextEditingValue _lastValue = TextEditingValue.empty;
  final List<_AnimationParticle> _particles = [];
  int _particleIdCounter = 0;

  @override
  void initState() {
    super.initState();
    _lastValue = widget.controller.value;
    widget.controller.addListener(_onTextChanged);
  }

  @override
  void didUpdateWidget(EditorInputAnimationOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onTextChanged);
      widget.controller.addListener(_onTextChanged);
      _lastValue = widget.controller.value;
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
      return;
    }

    final newValue = widget.controller.value;

    // Ignore composing
    if (newValue.composing.isValid) {
      _lastValue = newValue;
      return;
    }

    final textChanged = _lastValue.text != newValue.text;
    final selectionChanged = _lastValue.selection != newValue.selection;

    if (textChanged && widget.typedCharacterAnimationEnabled) {
      // Very basic diff to find inserted character
      final oldLen = _lastValue.text.length;
      final newLen = newValue.text.length;

      if (newLen > oldLen && (newLen - oldLen) <= 3) {
        // Find inserted char (approximate logic for prototype)
        // If the selection is collapsed, the insertion likely happened just before it.
        if (newValue.selection.isCollapsed &&
            newValue.selection.baseOffset > 0) {
          final insertedChar = newValue.text.substring(
            max(0, newValue.selection.baseOffset - (newLen - oldLen)),
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
  }

  RenderEditable? _findRenderEditable(RenderObject? root) {
    if (root == null) return null;
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
      if (renderObject == null || renderObject is! RenderBox) {
        return fallbackOffset;
      }

      final RenderEditable? renderEditable = _findRenderEditable(renderObject);
      if (renderEditable == null) {
        return fallbackOffset;
      }

      final selection = widget.controller.selection;
      if (!selection.isValid) {
        return fallbackOffset;
      }

      // Calculate relative to RenderEditable
      final caretRect = renderEditable.getLocalRectForCaret(
        TextPosition(offset: selection.baseOffset),
      );

      // Convert from RenderEditable coordinates to Screen coordinates
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
    Future.delayed(const Duration(milliseconds: 300), () {
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

    return Stack(
      children: [
        widget.child,
        // The overlay layer
        Positioned.fill(
          child: IgnorePointer(
            child: Stack(
              children: _particles.map((p) => _buildParticleWidget(p)).toList(),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildParticleWidget(_AnimationParticle particle) {
    return Positioned(
      left: particle.offsetX,
      top: particle.offsetY,
      child: TweenAnimationBuilder<double>(
        key: ValueKey(particle.id),
        tween: Tween(begin: 0.0, end: 1.0),
        duration: const Duration(milliseconds: 200),
        curve: Curves.easeOutCubic,
        builder: (context, value, child) {
          if (particle.isCursor) {
            // Cursor pulse
            return Opacity(
              opacity: 1.0 - value,
              child: Container(
                width: 2,
                height: 20,
                color: Colors.blueAccent.withAlpha(150),
              ),
            );
          } else {
            // Typed character slide up and fade
            return Opacity(
              opacity: 1.0 - value,
              child: Transform.translate(
                offset: Offset(0, -10 * value),
                child: Text(
                  particle.text,
                  style: const TextStyle(
                    fontSize: 16,
                    color: Colors.blueAccent,
                    fontWeight: FontWeight.bold,
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
