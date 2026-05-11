import 'package:flutter/material.dart';

import 'editor_input_animation_overlay.dart';
import 'smooth_scroll_wrapper.dart';

class EditorPanel extends StatefulWidget {
  final bool hasChapter;
  final TextEditingController textController;
  final ScrollController scrollController;
  final Function(String) onChanged;

  final bool inputAnimationEnabled;
  final bool typedCharacterAnimationEnabled;
  final bool cursorAnimationEnhanced;

  final bool smoothScrollingEnabled;
  final int smoothScrollDurationMs;

  final double editorFontSize;
  final double editorLineHeight;
  final double editorContentWidth;
  final String? activeChapterId;
  // Note: editorParagraphSpacing is currently deferred as TextField does not natively support paragraph spacing out-of-the-box easily without replacing the text rendering engine.

  const EditorPanel({
    super.key,
    required this.hasChapter,
    required this.textController,
    required this.scrollController,
    required this.onChanged,
    this.inputAnimationEnabled = false,
    this.typedCharacterAnimationEnabled = false,
    this.cursorAnimationEnhanced = false,
    this.smoothScrollingEnabled = true,
    this.smoothScrollDurationMs = 120,
    this.editorFontSize = 16.0,
    this.editorLineHeight = 1.6,
    this.editorContentWidth = 800.0,
    this.activeChapterId,
  });

  @override
  State<EditorPanel> createState() => _EditorPanelState();
}

class _EditorPanelState extends State<EditorPanel> {
  @override
  Widget build(BuildContext context) {
    if (!widget.hasChapter) {
      return const Padding(
        padding: EdgeInsets.all(16.0),
        child: Center(child: Text('无章节，请新建或选择章节。')),
      );
    }

    final textField = TextField(
      controller: widget.textController,
      scrollController: widget.scrollController,
      maxLines: null,
      expands: true,
      decoration: const InputDecoration(
        border: InputBorder.none,
        hintText: '开始你的创作...',
      ),
      style: TextStyle(
        fontSize: widget.editorFontSize,
        height: widget.editorLineHeight,
      ),
      onChanged: widget.onChanged,
    );

    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: widget.editorContentWidth),
        child: Padding(
          padding: const EdgeInsets.all(16.0),
          child: SmoothScrollWrapper(
            controller: widget.scrollController,
            smoothScrollingEnabled: widget.smoothScrollingEnabled,
            smoothScrollDurationMs: widget.smoothScrollDurationMs,
            child: EditorInputAnimationOverlay(
              controller: widget.textController,
              inputAnimationEnabled: widget.inputAnimationEnabled,
              typedCharacterAnimationEnabled:
                  widget.typedCharacterAnimationEnabled,
              cursorAnimationEnhanced: widget.cursorAnimationEnhanced,
              editorFontSize:
                  widget.editorFontSize, // Pass font size for particles
              activeChapterId: widget.activeChapterId,
              child: textField,
            ),
          ),
        ),
      ),
    );
  }
}
