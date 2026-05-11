import 'package:flutter/material.dart';
import '../../application/controllers/settings_controller.dart';
import '../../domain/models/settings.dart';
import '../../infrastructure/platform/linux_runtime_environment.dart';
import '../../infrastructure/logging/app_logger.dart';
import '../../infrastructure/diagnostics/diagnostic_bundle_service.dart';

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

  late LocalSettings _draftLocalSettings;
  late SyncableSettings _draftSyncableSettings;
  bool _isDirty = false;
  bool _isSaving = false;

  final List<String> _categories = [
    '通用',
    '编辑器',
    'AI 设置',
    '纠错',
    '同步 (Beta)',
    '日志/诊断',
  ];

  @override
  void initState() {
    super.initState();
    _draftLocalSettings = widget.controller.localSettings;
    _draftSyncableSettings = widget.controller.syncableSettings;
    widget.controller.addListener(_onControllerUpdated);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onControllerUpdated);
    super.dispose();
  }

  void _onControllerUpdated() {
    if (mounted) {
      if (!_isDirty && !widget.controller.isLoading) {
        setState(() {
          _draftLocalSettings = widget.controller.localSettings;
          _draftSyncableSettings = widget.controller.syncableSettings;
        });
      }
    }
  }

  void _updateDraftLocal(LocalSettings newSettings) {
    setState(() {
      _draftLocalSettings = newSettings;
      _isDirty =
          _draftLocalSettings != widget.controller.localSettings ||
          _draftSyncableSettings != widget.controller.syncableSettings;
    });
  }

  void _updateDraftSyncable(SyncableSettings newSettings) {
    setState(() {
      _draftSyncableSettings = newSettings;
      _isDirty =
          _draftLocalSettings != widget.controller.localSettings ||
          _draftSyncableSettings != widget.controller.syncableSettings;
    });
  }

  Future<void> _saveSettings() async {
    setState(() {
      _isSaving = true;
    });

    widget.controller.updateLocalSettings(_draftLocalSettings);
    widget.controller.updateSyncableSettings(_draftSyncableSettings);
    await widget.controller.save();

    if (mounted) {
      setState(() {
        _isSaving = false;
        _isDirty = false;
      });
    }
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
      case 5:
        return _buildLogSettings();
      default:
        return const Center(child: Text('Unknown category'));
    }
  }

  Widget _buildGeneralSettings() {
    return ListView(
      children: [
        const Text(
          '基本设置',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: _draftSyncableSettings.startupBehavior,
          decoration: const InputDecoration(
            labelText: '启动行为',
            border: OutlineInputBorder(),
          ),
          items: const [
            DropdownMenuItem(value: 'projectHome', child: Text('作品管理页')),
            DropdownMenuItem(
              value: 'continueLastSession',
              child: Text('继续上次写作'),
            ),
          ],
          onChanged: (val) {
            if (val != null) {
              _updateDraftSyncable(
                _draftSyncableSettings.copyWith(startupBehavior: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftLocalSettings.workspacePath,
          readOnly: true,
          decoration: const InputDecoration(
            labelText: '工作区路径 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftLocalSettings.deviceName,
          decoration: const InputDecoration(
            labelText: '设备名称',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            _updateDraftLocal(_draftLocalSettings.copyWith(deviceName: val));
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
          value: _draftSyncableSettings.autoSaveEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(autoSaveEnabled: val),
            );
          },
        ),
        ListTile(
          title: const Text('自动保存间隔 (秒)'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: _draftSyncableSettings.autoSaveIntervalSeconds
                  .toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final intVal = int.tryParse(val);
                if (intVal != null) {
                  _updateDraftSyncable(
                    _draftSyncableSettings.copyWith(
                      autoSaveIntervalSeconds: intVal,
                    ),
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
    return ListView(
      children: [
        const Text(
          '输入动效',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('启用输入动效'),
          subtitle: const Text('包含打字与光标反馈效果'),
          value: _draftSyncableSettings.inputAnimationEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(inputAnimationEnabled: val),
            );
          },
        ),
        if (_draftSyncableSettings.inputAnimationEnabled) ...[
          SwitchListTile(
            title: const Text('字符吐出动画'),
            value: _draftSyncableSettings.typedCharacterAnimationEnabled,
            onChanged: (val) {
              _updateDraftSyncable(
                _draftSyncableSettings.copyWith(
                  typedCharacterAnimationEnabled: val,
                ),
              );
            },
          ),
          SwitchListTile(
            title: const Text('光标移动反馈'),
            value: _draftSyncableSettings.cursorAnimationEnhanced,
            onChanged: (val) {
              _updateDraftSyncable(
                _draftSyncableSettings.copyWith(cursorAnimationEnhanced: val),
              );
            },
          ),
        ],
        const Divider(),
        const SizedBox(height: 16),
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
              initialValue: _draftSyncableSettings.editorFontSize.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  _updateDraftSyncable(
                    _draftSyncableSettings.copyWith(editorFontSize: doubleVal),
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
              initialValue: _draftSyncableSettings.editorLineHeight.toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  _updateDraftSyncable(
                    _draftSyncableSettings.copyWith(
                      editorLineHeight: doubleVal,
                    ),
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
              initialValue: _draftSyncableSettings.editorParagraphSpacing
                  .toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  _updateDraftSyncable(
                    _draftSyncableSettings.copyWith(
                      editorParagraphSpacing: doubleVal,
                    ),
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
              initialValue: _draftSyncableSettings.editorContentWidth
                  .toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final doubleVal = double.tryParse(val);
                if (doubleVal != null) {
                  _updateDraftSyncable(
                    _draftSyncableSettings.copyWith(
                      editorContentWidth: doubleVal,
                    ),
                  );
                }
              },
            ),
          ),
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: _draftSyncableSettings.themeMode,
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
              _updateDraftSyncable(
                _draftSyncableSettings.copyWith(themeMode: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('打字机模式'),
          value: _draftSyncableSettings.typewriterModeEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(typewriterModeEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('专注模式'),
          value: _draftSyncableSettings.focusModeEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(focusModeEnabled: val),
            );
          },
        ),
        const SizedBox(height: 32),
        const Text(
          '平滑滚动',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('平滑滚动'),
          value: _draftSyncableSettings.smoothScrollingEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(smoothScrollingEnabled: val),
            );
          },
        ),
        ListTile(
          title: const Text('平滑滚动时长 ms'),
          trailing: SizedBox(
            width: 100,
            child: TextFormField(
              initialValue: _draftSyncableSettings.smoothScrollDurationMs
                  .toString(),
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onChanged: (val) {
                final intVal = int.tryParse(val);
                if (intVal != null) {
                  // restrict range from 80 to 500 ms
                  final clampedVal = intVal.clamp(80, 500);
                  _updateDraftSyncable(
                    _draftSyncableSettings.copyWith(
                      smoothScrollDurationMs: clampedVal,
                    ),
                  );
                }
              },
            ),
          ),
        ),
        const SizedBox(height: 32),
        const Text(
          '输入动效 (Beta)',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const Padding(
          padding: EdgeInsets.symmetric(horizontal: 16.0),
          child: Text(
            '实验功能。若输入法异常，请关闭或开启 IME Safe Mode。',
            style: TextStyle(color: Colors.grey),
          ),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('启用整体输入动效'),
          value: _draftSyncableSettings.inputAnimationEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(inputAnimationEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('打字弹跃动效 (吐字/删除)'),
          value: _draftSyncableSettings.typedCharacterAnimationEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(
                typedCharacterAnimationEnabled: val,
              ),
            );
          },
        ),
        SwitchListTile(
          title: const Text('增强光标平滑移动'),
          value: _draftSyncableSettings.cursorAnimationEnhanced,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(cursorAnimationEnhanced: val),
            );
          },
        ),
      ],
    );
  }

  Widget _buildAISettings() {
    return ListView(
      children: [
        const Text(
          '默认 AI 模型',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: _draftSyncableSettings.defaultAIProvider,
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
              _updateDraftSyncable(
                _draftSyncableSettings.copyWith(defaultAIProvider: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.defaultAIModel,
          decoration: const InputDecoration(
            labelText: '默认模型名称',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(defaultAIModel: val),
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
          initialValue: _draftSyncableSettings.deepSeekBaseUrl,
          decoration: const InputDecoration(
            labelText: 'Base URL',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(deepSeekBaseUrl: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.deepSeekApiKey,
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
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(deepSeekApiKey: val),
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
          value: _draftSyncableSettings.aiToolsEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(aiToolsEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('启用 AI 思考模式'),
          value: _draftSyncableSettings.aiThinkingModeEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(aiThinkingModeEnabled: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.aiPromptTemplateVersion
              .toString(),
          readOnly: true,
          decoration: const InputDecoration(
            labelText: 'Prompt 模板版本 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.aiToolDefinitionVersion
              .toString(),
          readOnly: true,
          decoration: const InputDecoration(
            labelText: 'Tool Definition 版本 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.aiSerializerVersion.toString(),
          readOnly: true,
          decoration: const InputDecoration(
            labelText: 'Serializer 版本 (只读)',
            border: OutlineInputBorder(),
          ),
        ),
        const Divider(),
        const SizedBox(height: 16),
        const Text('诊断与支持', style: TextStyle(fontWeight: FontWeight.bold)),
        const SizedBox(height: 8),
        SelectableText('应用日志目录：${AppLogger.logDirPath ?? "未初始化"}'),
        const SizedBox(height: 16),
        ElevatedButton.icon(
          onPressed: () async {
            try {
              AppLogger.pauseLogging();
              final path = await DiagnosticBundleService.exportDiagnostics(
                widget.controller,
              );
              if (mounted) {
                ScaffoldMessenger.of(
                  context,
                ).showSnackBar(SnackBar(content: Text('诊断包已导出至：$path')));
              }
            } catch (e, st) {
              AppLogger.resumeLogging();
              AppLogger.error('Failed to export diagnostics', e, st);
              if (mounted) {
                ScaffoldMessenger.of(
                  context,
                ).showSnackBar(SnackBar(content: Text('导出失败：$e')));
              }
            } finally {
              AppLogger.resumeLogging();
            }
          },
          icon: const Icon(Icons.bug_report),
          label: const Text('导出安全诊断包'),
        ),
        const SizedBox(height: 8),
        const Text(
          '导出诊断包将收集应用状态和最近日志以供排查问题，且已自动隐藏所有敏感信息。',
          style: TextStyle(fontSize: 12, color: Colors.grey),
        ),
      ],
    );
  }

  Widget _buildCorrectionSettings() {
    return ListView(
      children: [
        const Text(
          '自动纠错',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        SwitchListTile(
          title: const Text('启用拼写/语法纠错'),
          value: _draftSyncableSettings.correctionEnabled,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(correctionEnabled: val),
            );
          },
        ),
      ],
    );
  }

  Widget _buildLogSettings() {
    final env = LinuxRuntimeEnvironment();
    final bool showLinuxImeWarning =
        env.isLinux &&
        env.isWayland &&
        (env.gtkImModule == 'fcitx' || env.qtImModule == 'fcitx');

    return ListView(
      children: [
        const Text(
          '日志/诊断',
          style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 16),
        if (env.isLinux) ...[
          const Text(
            'Linux 环境诊断信息',
            style: TextStyle(fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 8),
          SelectableText(
            env.summaryText,
            style: const TextStyle(fontSize: 12, fontFamily: 'monospace'),
          ),
          if (showLinuxImeWarning) ...[
            const SizedBox(height: 8),
            Container(
              padding: const EdgeInsets.all(8),
              color: Colors.amber.withValues(alpha: 0.2),
              child: const Row(
                children: [
                  Icon(Icons.warning, color: Colors.orange),
                  SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      '警告：检测到 Wayland 下强制设置 GTK_IM_MODULE/QT_IM_MODULE。fcitx5 官方推荐 KDE Wayland 使用 Wayland 输入法前端，这些变量可能导致候选框闪烁。建议 unset GTK_IM_MODULE、QT_IM_MODULE、SDL_IM_MODULE，只保留 XMODIFIERS=@im=fcitx。',
                      style: TextStyle(color: Colors.deepOrange, fontSize: 13),
                    ),
                  ),
                ],
              ),
            ),
          ],
          const Divider(),
          const SizedBox(height: 16),
        ],
        SwitchListTile(
          title: const Text('启用日志'),
          value: _draftLocalSettings.loggingEnabled,
          onChanged: (val) {
            _updateDraftLocal(
              _draftLocalSettings.copyWith(loggingEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('启用调试日志'),
          value: _draftLocalSettings.debugLoggingEnabled,
          onChanged: (val) {
            _updateDraftLocal(
              _draftLocalSettings.copyWith(debugLoggingEnabled: val),
            );
          },
        ),
        SwitchListTile(
          title: const Text('启用性能日志'),
          value: _draftLocalSettings.performanceLoggingEnabled,
          onChanged: (val) {
            _updateDraftLocal(
              _draftLocalSettings.copyWith(performanceLoggingEnabled: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftLocalSettings.slowOperationThresholdMs.toString(),
          decoration: const InputDecoration(
            labelText: '慢操作阈值 (ms)',
            border: OutlineInputBorder(),
          ),
          keyboardType: TextInputType.number,
          onChanged: (val) {
            final ms = int.tryParse(val);
            if (ms != null) {
              _updateDraftLocal(
                _draftLocalSettings.copyWith(slowOperationThresholdMs: ms),
              );
            }
          },
        ),
        const SizedBox(height: 24),
        TextFormField(
          initialValue: _draftLocalSettings.workspacePath.isNotEmpty
              ? '${_draftLocalSettings.workspacePath}/app-meta/logs'
              : '未初始化',
          readOnly: true,
          decoration: const InputDecoration(
            labelText: '日志目录路径',
            border: OutlineInputBorder(),
          ),
        ),
      ],
    );
  }

  Widget _buildSyncSettings() {
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
          initialValue: _draftSyncableSettings.githubRepoUrl,
          decoration: const InputDecoration(
            labelText: 'Repository URL',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(githubRepoUrl: val),
            );
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.githubBranch,
          decoration: const InputDecoration(
            labelText: 'Branch',
            border: OutlineInputBorder(),
          ),
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(githubBranch: val),
            );
          },
        ),
        const SizedBox(height: 16),
        DropdownButtonFormField<String>(
          initialValue: _draftSyncableSettings.githubSyncMethod,
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
              _updateDraftSyncable(
                _draftSyncableSettings.copyWith(githubSyncMethod: val),
              );
            }
          },
        ),
        const SizedBox(height: 16),
        TextFormField(
          initialValue: _draftSyncableSettings.githubToken,
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
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(githubToken: val),
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
          value: _draftSyncableSettings.syncApiKeysInPlaintext,
          onChanged: (val) {
            _updateDraftSyncable(
              _draftSyncableSettings.copyWith(syncApiKeysInPlaintext: val),
            );
          },
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth < 700) {
          // Mobile Layout
          return Dialog.fullscreen(
            child: Scaffold(
              appBar: AppBar(title: const Text('设置')),
              body: Column(
                children: [
                  SizedBox(
                    height: 50,
                    child: ListView.builder(
                      scrollDirection: Axis.horizontal,
                      itemCount: _categories.length,
                      itemBuilder: (context, index) {
                        final isSelected = _selectedIndex == index;
                        return InkWell(
                          onTap: () {
                            setState(() {
                              _selectedIndex = index;
                            });
                          },
                          child: Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 16.0,
                            ),
                            alignment: Alignment.center,
                            decoration: BoxDecoration(
                              border: Border(
                                bottom: BorderSide(
                                  color: isSelected
                                      ? Theme.of(context).colorScheme.primary
                                      : Colors.transparent,
                                  width: 2,
                                ),
                              ),
                            ),
                            child: Text(
                              _categories[index],
                              style: TextStyle(
                                color: isSelected
                                    ? Theme.of(context).colorScheme.primary
                                    : Theme.of(
                                        context,
                                      ).textTheme.bodyMedium?.color,
                                fontWeight: isSelected
                                    ? FontWeight.bold
                                    : FontWeight.normal,
                              ),
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.all(16.0),
                      child: _buildContent(),
                    ),
                  ),
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
                        if (_isDirty)
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
                          onPressed: _isDirty && !_isSaving
                              ? _saveSettings
                              : null,
                          child: _isSaving
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
          );
        }

        // Desktop Layout
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
                            top: BorderSide(
                              color: Theme.of(context).dividerColor,
                            ),
                          ),
                        ),
                        child: Row(
                          mainAxisAlignment: MainAxisAlignment.end,
                          children: [
                            if (_isDirty)
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
                              onPressed: _isDirty && !_isSaving
                                  ? _saveSettings
                                  : null,
                              child: _isSaving
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
      },
    );
  }
}
