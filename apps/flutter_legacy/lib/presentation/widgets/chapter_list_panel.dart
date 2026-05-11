import 'package:flutter/material.dart';
import '../../domain/models/chapter.dart';

class ChapterListPanel extends StatelessWidget {
  final List<Chapter> chapters;
  final Chapter? selectedChapter;
  final VoidCallback onAddChapter;
  final Function(Chapter) onSelectChapter;

  const ChapterListPanel({
    super.key,
    required this.chapters,
    required this.selectedChapter,
    required this.onAddChapter,
    required this.onSelectChapter,
  });

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 250,
      child: Column(
        children: [
          ListTile(
            title: const Text(
              '默认项目 / 默认卷',
              style: TextStyle(fontWeight: FontWeight.bold),
            ),
            trailing: IconButton(
              icon: const Icon(Icons.add),
              onPressed: onAddChapter,
            ),
          ),
          const Divider(),
          Expanded(
            child: ListView.builder(
              itemCount: chapters.length,
              itemBuilder: (context, index) {
                final chap = chapters[index];
                final isSelected = selectedChapter?.id == chap.id;
                return ListTile(
                  selected: isSelected,
                  title: Text(chap.title),
                  subtitle: Text('${chap.wordCount} 字'),
                  onTap: () => onSelectChapter(chap),
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}
