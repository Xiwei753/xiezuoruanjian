# 工作区格式定义

工作区格式定义了存储写作文件、元数据、设置和缓存的精确目录结构和文件格式。它是共享 Rust 核心和所有原生客户端的唯一事实来源。

```
workspace/
├─ workspace_manifest.json       # 工作区基本信息
├─ app-meta/                     # 全局元数据和设置
│  ├─ settings/
│  │  ├─ settings.local.json     # 设备特定设置（不同步）
│  │  └─ settings.sync.json      # 跨设备设置（同步）
│  └─ logs/                      # 应用日志
├─ projects/
│  └─ <project_id>/              # 项目目录
│     ├─ project.json            # 项目元数据
│     ├─ volumes/
│     │  └─ <volume_id>/         # 卷目录
│     │     ├─ volume.json       # 卷元数据
│     │     └─ chapters/
│     │        └─ <chapter_id>/  # 章节目录
│     │           ├─ chapter.md  # 正文
│     │           └─ chapter.meta.json # 章节元数据
│     └─ characters/             # 角色卡（如适用）
├─ backups/                      # 备份文件
├─ trash/                        # 已删除文件
└─ sqlite_cache/                 # 可重建缓存（非事实来源）
```
