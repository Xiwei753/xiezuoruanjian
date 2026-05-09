import 'package:flutter/material.dart';
import '../../domain/models/chapter.dart';

class ChapterInfoPanel extends StatelessWidget {
  final String workspacePath;
  final Chapter? chapter;
  final bool isSaving;
  final VoidCallback onEditTitle;

  const ChapterInfoPanel({
    super.key,
    required this.workspacePath,
    required this.chapter,
    required this.isSaving,
    required this.onEditTitle,
  });

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 250,
      child: Padding(
        padding: const EdgeInsets.all(16.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('工作区', style: Theme.of(context).textTheme.titleSmall),
            Text(
              workspacePath,
              style: const TextStyle(fontSize: 10, color: Colors.grey),
            ),
            const SizedBox(height: 24),
            if (chapter != null) ...[
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Text('章节信息', style: Theme.of(context).textTheme.titleLarge),
                  IconButton(
                    icon: const Icon(Icons.edit, size: 16),
                    onPressed: onEditTitle,
                  ),
                ],
              ),
              const SizedBox(height: 16),
              Text(
                '标题: ${chapter!.title}',
                style: const TextStyle(fontWeight: FontWeight.bold),
              ),
              const SizedBox(height: 8),
              Text('字数: ${chapter!.wordCount}'),
              const SizedBox(height: 8),
              Text('Hash:'),
              Text(
                chapter!.contentHash.isEmpty
                    ? 'N/A'
                    : '${chapter!.contentHash.substring(0, 16)}...',
                style: const TextStyle(fontSize: 12, color: Colors.grey),
              ),
              const SizedBox(height: 24),
              Row(
                children: [
                  Icon(
                    isSaving ? Icons.sync : Icons.check_circle,
                    color: isSaving ? Colors.orange : Colors.green,
                    size: 16,
                  ),
                  const SizedBox(width: 8),
                  Text(isSaving ? '保存中...' : '已保存'),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}
