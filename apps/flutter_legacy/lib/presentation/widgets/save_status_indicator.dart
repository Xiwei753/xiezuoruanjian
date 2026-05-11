import 'package:flutter/material.dart';
import '../../application/controllers/workspace_controller.dart';

class SaveStatusIndicator extends StatelessWidget {
  final WorkspaceController controller;

  const SaveStatusIndicator({super.key, required this.controller});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        if (controller.isSaving) {
          return const Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              SizedBox(
                width: 12,
                height: 12,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
              SizedBox(width: 8),
              Text(
                '保存中...',
                style: TextStyle(color: Colors.grey, fontSize: 12),
              ),
            ],
          );
        }

        if (controller.lastSaveError != null) {
          return Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.error_outline, color: Colors.red, size: 14),
              const SizedBox(width: 4),
              Text(
                '保存失败',
                style: const TextStyle(color: Colors.red, fontSize: 12),
                overflow: TextOverflow.ellipsis,
              ),
            ],
          );
        }

        if (controller.isDirty) {
          return const Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.edit, color: Colors.orange, size: 14),
              SizedBox(width: 4),
              Text('未保存', style: TextStyle(color: Colors.orange, fontSize: 12)),
            ],
          );
        }

        if (controller.lastSavedAt != null &&
            !controller.isDirty &&
            controller.selectedChapter != null) {
          final timeStr =
              "${controller.lastSavedAt!.hour.toString().padLeft(2, '0')}:${controller.lastSavedAt!.minute.toString().padLeft(2, '0')}:${controller.lastSavedAt!.second.toString().padLeft(2, '0')}";
          return Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(
                Icons.check_circle_outline,
                color: Colors.green,
                size: 14,
              ),
              const SizedBox(width: 4),
              Text(
                '已保存 $timeStr',
                style: const TextStyle(color: Colors.green, fontSize: 12),
              ),
            ],
          );
        }

        return const SizedBox.shrink();
      },
    );
  }
}
