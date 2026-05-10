import 'package:flutter/material.dart';
import '../../application/controllers/settings_controller.dart';

class SettingsDialog extends StatefulWidget {
  final SettingsController controller;

  const SettingsDialog({super.key, required this.controller});

  @override
  State<SettingsDialog> createState() => _SettingsDialogState();
}

class _SettingsDialogState extends State<SettingsDialog> {
  int _selectedIndex = 0;
  bool _obscureDeepSeekApiKey = true;
  bool _obscureGithubToken = true;

  final List<String> _categories = ['通用', '编辑器', 'AI 设置', '纠错', '同步 (Beta)'];

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onControllerUpdated);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onControllerUpdated);
    super.dispose();
  }

  void _onControllerUpdated() {
    setState(() {});
  }

  Widget _buildContent() {
    if (widget.controller.isLoading) {
      return const Center(child: CircularProgressIndicator());
    }

    if (widget.controller.errorMessage != null) {
      return Center(
        child: Text(
          widget.controller.errorMessage!,
          style: const TextStyle(color: Colors.red),
        ),
      );
    }

    switch (_selectedIndex) {
      case 0:
        return _buildGeneralSettings();
      case 1:
        return _buildEditorSettings();
      case 2:
        return _buildAISettings();
      case 3:
        return _buildCorrectionSettings();
      case 4:
        return _buildSyncSettings();
      default:
        return const Center(child: Text('Unknown category'));
    }
  }

  Widget _buildGeneralSettings() {
    final local = widget.controller.localSettings;
    final syncable = widget.controller.syncableSettings;

    return ListView(
      children: [
        const Text(
          '基本设置',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: local.workspacePath,
          readOnly: true,
          decoration: const InputDecoration(
            labelText: '工作区路径 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: local.deviceName,
          decoration: const InputDecoration(
            labelText: '设备名称',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            widget.controller.updateLocalSettings(
              local.copyWith(deviceName: val),
            );
          },
        ),
        const SizedBox(height: 32),
        const Text(
          '自动保存',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('启用自动保存'),
          value: syncable.autoSaveEnabled,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(autoSaveEnabled: val),
            );
          },
        ),
        ListTile(
          title: const Text('自动保存间隔 (秒)'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: syncable.autoSaveIntervalSeconds.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final intVal = int.tryParse(val);
                if (intVal != null) {
                  widget.controller.updateSyncableSettings(
                    syncable.copyWith(autoSaveIntervalSeconds: intVal),
                  );
                }
              },
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildEditorSettings() {
    final syncable = widget.controller.syncableSettings;

    return ListView(
      children: [
        const Text(
          '编辑器显示',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        ListTile(
          title: const Text('字体大小'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: syncable.editorFontSize.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  widget.controller.updateSyncableSettings(
                    syncable.copyWith(editorFontSize: doubleVal),
                  );
                }
              },
            ),
          ),
        ),
        ListTile(
          title: const Text('行高'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: syncable.editorLineHeight.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  widget.controller.updateSyncableSettings(
                    syncable.copyWith(editorLineHeight: doubleVal),
                  );
                }
              },
            ),
          ),
        ),
        ListTile(
          title: const Text('段落间距'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: syncable.editorParagraphSpacing.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  widget.controller.updateSyncableSettings(
                    syncable.copyWith(editorParagraphSpacing: doubleVal),
                  );
                }
              },
            ),
          ),
        ),
        ListTile(
          title: const Text('内容宽度'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: syncable.editorContentWidth.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  widget.controller.updateSyncableSettings(
                    syncable.copyWith(editorContentWidth: doubleVal),
                  );
                }
              },
            ),
          ),
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: syncable.themeMode,
          decoration: const InputDecoration(
            labelText: '主题模式',
            border: OutlineInputBorder(),
          ),
          items: const [
            DropdownMenuItem(value: 'system', child: Text('跟随系统')),
            DropdownMenuItem(value: 'light', child: Text('浅色')),
            DropdownMenuItem(value: 'dark', child: Text('深色')),
          ],
          onChanged: (val) {
            if (val != null) {
              widget.controller.updateSyncableSettings(
                syncable.copyWith(themeMode: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('打字机模式'),
          value: syncable.typewriterModeEnabled,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(typewriterModeEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('专注模式'),
          value: syncable.focusModeEnabled,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(focusModeEnabled: val),
            );
          },
        ),
      ],
    );
  }

  Widget _buildAISettings() {
    final syncable = widget.controller.syncableSettings;

    return ListView(
      children: [
        const Text(
          '默认 AI 模型',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: syncable.defaultAIProvider,
          decoration: const InputDecoration(
            labelText: 'AI 提供商',
            border: OutlineInputBorder(),
          ),
          items: const [
            DropdownMenuItem(value: 'deepseek', child: Text('DeepSeek')),
            DropdownMenuItem(value: 'mock', child: Text('Mock (Test)')),
          ],
          onChanged: (val) {
            if (val != null) {
              widget.controller.updateSyncableSettings(
                syncable.copyWith(defaultAIProvider: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.defaultAIModel,
          decoration: const InputDecoration(
            labelText: '默认模型名称',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(defaultAIModel: val),
            );
          },
        ),
        const SizedBox(height: 32),
        const Text(
          'DeepSeek 配置',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.deepSeekBaseUrl,
          decoration: const InputDecoration(
            labelText: 'Base URL',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(deepSeekBaseUrl: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.deepSeekApiKey,
          obscureText: _obscureDeepSeekApiKey,
          decoration: InputDecoration(
            labelText: 'API Key',
            border: const OutlineInputBorder(),
            suffixIcon: IconButton(
              icon: Icon(
                _obscureDeepSeekApiKey
                    ? Icons.visibility
                    : Icons.visibility_off,
              ),
              onPressed: () {
                setState(() {
                  _obscureDeepSeekApiKey = !_obscureDeepSeekApiKey;
                });
              },
            ),
          ),
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(deepSeekApiKey: val),
            );
          },
        ),
        const Padding(
          padding: EdgeInsets.only(top: 8.0, left: 4.0),
          child: Text(
            '此密钥会随私人文稿仓库同步保存。请确认该仓库为私有仓库，并理解 Git 历史可能长期保留已提交的密钥。如密钥泄露，请在对应服务商后台重置。',
            style: TextStyle(fontSize: 12, color: Colors.grey),
          ),
        ),
        const SizedBox(height: 32),
        const Text(
          '高级特性',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('启用 AI Tools'),
          value: syncable.aiToolsEnabled,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(aiToolsEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('启用 AI 思考模式'),
          value: syncable.aiThinkingModeEnabled,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(aiThinkingModeEnabled: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.aiPromptTemplateVersion.toString(),
          readOnly: true,
          decoration: const InputDecoration(
            labelText: 'Prompt 模板版本 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.aiToolDefinitionVersion.toString(),
          readOnly: true,
          decoration: const InputDecoration(
            labelText: 'Tool Definition 版本 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.aiSerializerVersion.toString(),
          readOnly: true,
          decoration: const InputDecoration(
            labelText: 'Serializer 版本 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
      ],
    );
  }

  Widget _buildCorrectionSettings() {
    final syncable = widget.controller.syncableSettings;

    return ListView(
      children: [
        const Text(
          '自动纠错',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('启用拼写/语法纠错'),
          value: syncable.correctionEnabled,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(correctionEnabled: val),
            );
          },
        ),
      ],
    );
  }

  Widget _buildSyncSettings() {
    final syncable = widget.controller.syncableSettings;

    return ListView(
      children: [
        const Text(
          'GitHub 同步设置',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 8),
        const Text(
          '这里配置的是私人文稿仓库，不是当前软件源码仓库。未来正文、设置和 AI 配置会通过该私人仓库同步。',
          style: TextStyle(fontSize: 13, color: Colors.blueAccent),
        ),
        const SizedBox(height: 24),
        TextFormField(
          initialValue: syncable.githubRepoUrl,
          decoration: const InputDecoration(
            labelText: 'Repository URL',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(githubRepoUrl: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.githubBranch,
          decoration: const InputDecoration(
            labelText: 'Branch',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(githubBranch: val),
            );
          },
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: syncable.githubSyncMethod,
          decoration: const InputDecoration(
            labelText: '同步方式',
            border: OutlineInputBorder(),
          ),
          items: const [
            DropdownMenuItem(value: 'ssh', child: Text('SSH')),
            DropdownMenuItem(value: 'https', child: Text('HTTPS / Token')),
          ],
          onChanged: (val) {
            if (val != null) {
              widget.controller.updateSyncableSettings(
                syncable.copyWith(githubSyncMethod: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: syncable.githubToken,
          obscureText: _obscureGithubToken,
          decoration: InputDecoration(
            labelText: 'GitHub Token',
            border: const OutlineInputBorder(),
            suffixIcon: IconButton(
              icon: Icon(
                _obscureGithubToken ? Icons.visibility : Icons.visibility_off,
              ),
              onPressed: () {
                setState(() {
                  _obscureGithubToken = !_obscureGithubToken;
                });
              },
            ),
          ),
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(githubToken: val),
            );
          },
        ),
        const Padding(
          padding: EdgeInsets.only(top: 8.0, left: 4.0),
          child: Text(
            '此密钥会随私人文稿仓库同步保存。请确认该仓库为私有仓库，并理解 Git 历史可能长期保留已提交的密钥。如密钥泄露，请在对应服务商后台重置。',
            style: TextStyle(fontSize: 12, color: Colors.grey),
          ),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('允许明文同步 API Keys 和 Tokens'),
          subtitle: const Text('当前设计为明文存入 settings.sync.json。'),
          value: syncable.syncApiKeysInPlaintext,
          onChanged: (val) {
            widget.controller.updateSyncableSettings(
              syncable.copyWith(syncApiKeysInPlaintext: val),
            );
          },
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      child: Container(
        width: 800,
        height: 600,
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(borderRadius: BorderRadius.circular(12)),
        child: Row(
          children: [
            // Sidebar
            Container(
              width: 200,
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const Padding(
                    padding: EdgeInsets.all(16.0),
                    child: Text(
                      '设置',
                      style: TextStyle(
                        fontSize: 20,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  Expanded(
                    child: ListView.builder(
                      itemCount: _categories.length,
                      itemBuilder: (context, index) {
                        return ListTile(
                          title: Text(_categories[index]),
                          selected: _selectedIndex == index,
                          onTap: () {
                            setState(() {
                              _selectedIndex = index;
                            });
                          },
                        );
                      },
                    ),
                  ),
                ],
              ),
            ),
            // Content area
            Expanded(
              child: Column(
                children: [
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.all(24.0),
                      child: _buildContent(),
                    ),
                  ),
                  // Bottom bar for saving
                  Container(
                    padding: const EdgeInsets.all(16.0),
                    decoration: BoxDecoration(
                      border: Border(
                        top: BorderSide(color: Theme.of(context).dividerColor),
                      ),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.end,
                      children: [
                        if (widget.controller.isDirty)
                          const Padding(
                            padding: EdgeInsets.only(right: 16.0),
                            child: Text(
                              '有未保存的更改',
                              style: TextStyle(
                                color: Colors.orange,
                                fontSize: 12,
                              ),
                            ),
                          ),
                        TextButton(
                          onPressed: () => Navigator.pop(context),
                          child: const Text('关闭'),
                        ),
                        const SizedBox(width: 16),
                        ElevatedButton(
                          onPressed:
                              widget.controller.isDirty &&
                                  !widget.controller.isSaving
                              ? () async {
                                  await widget.controller.save();
                                }
                              : null,
                          child: widget.controller.isSaving
                              ? const SizedBox(
                                  width: 16,
                                  height: 16,
                                  child: CircularProgressIndicator(
                                    strokeWidth: 2,
                                  ),
                                )
                              : const Text('保存'),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
