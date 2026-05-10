import 'package:flutter/material.dart';

import 'editor_input_animation_overlay.dart';

class EditorPanel extends StatefulWidget {
  final bool hasChapter;
  final TextEditingController textController;
  final ScrollController scrollController;
  final Function(String) onChanged;

  final bool inputAnimationEnabled;
  final bool typedCharacterAnimationEnabled;
  final bool cursorAnimationEnhanced;

  const EditorPanel({
    super.key,
    required this.hasChapter,
    required this.textController,
    required this.scrollController,
    required this.onChanged,
    this.inputAnimationEnabled = false,
    this.typedCharacterAnimationEnabled = false,
    this.cursorAnimationEnhanced = false,
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
      style: const TextStyle(fontSize: 16, height: 1.6),
      onChanged: widget.onChanged,
    );

    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: EditorInputAnimationOverlay(
        controller: widget.textController,
        inputAnimationEnabled: widget.inputAnimationEnabled,
        typedCharacterAnimationEnabled: widget.typedCharacterAnimationEnabled,
        cursorAnimationEnhanced: widget.cursorAnimationEnhanced,
        child: textField,
      ),
    );
  }
}
