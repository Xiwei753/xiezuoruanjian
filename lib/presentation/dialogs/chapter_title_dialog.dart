import 'package:flutter/material.dart';

class ChapterTitleDialog extends StatefulWidget {
  final String title;
  final String initialValue;
  final String confirmText;

  const ChapterTitleDialog({
    super.key,
    required this.title,
    this.initialValue = '',
    this.confirmText = '确认',
  });

  @override
  State<ChapterTitleDialog> createState() => _ChapterTitleDialogState();
}

class _ChapterTitleDialogState extends State<ChapterTitleDialog> {
  late String _inputTitle;

  @override
  void initState() {
    super.initState();
    _inputTitle = widget.initialValue;
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.title),
      content: TextFormField(
        initialValue: _inputTitle,
        autofocus: true,
        decoration: const InputDecoration(hintText: '章节标题'),
        onChanged: (val) => _inputTitle = val,
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('取消'),
        ),
        TextButton(
          onPressed: () => Navigator.pop(context, _inputTitle),
          child: Text(widget.confirmText),
        ),
      ],
    );
  }
}
