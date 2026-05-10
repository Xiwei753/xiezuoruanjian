import 'package:flutter/material.dart';

class EditorPanel extends StatefulWidget {
  final bool hasChapter;
  final TextEditingController textController;
  final Function(String) onChanged;

  const EditorPanel({
    super.key,
    required this.hasChapter,
    required this.textController,
    required this.onChanged,
  });

  @override
  State<EditorPanel> createState() => _EditorPanelState();
}

class _EditorPanelState extends State<EditorPanel> {
  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: !widget.hasChapter
          ? const Center(child: Text('无章节，请新建或选择章节。'))
          : TextField(
              controller: widget.textController,
              maxLines: null,
              expands: true,
              decoration: const InputDecoration(
                border: InputBorder.none,
                hintText: '开始你的创作...',
              ),
              style: const TextStyle(fontSize: 16, height: 1.6),
              onChanged: widget.onChanged,
            ),
    );
  }
}
