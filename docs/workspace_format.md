# 工作区格式定义

Status: active
Last verified: 2026-06-23
Truth source: protocol
Supersedes: None

工作区格式定义了存储写作文件、元数据、设置和缓存的精确目录结构和文件格式。它是共享 Rust 核心和所有原生客户端的唯一事实来源。

```
workspace/
├─ workspace_manifest.json       # 工作区基本信息
├─ app-meta/                     # 全局元数据和设置
│  ├─ settings/
│  │  ├─ settings.local.json     # 设备特定设置（不同步）
│  │  └─ settings.sync.json      # 跨设备设置（同步）
│  ├─ sync/                      # 同步状态与配置
│  │  ├─ sync_config.json        # 同步配置（enabled、backend_type、remote_url、transport、branch）
│  │  ├─ sync_secrets.local.json # 敏感信息（GitHub Token、SSH 私钥，不同步）
│  │  ├─ sync_state.json         # 同步状态（不同步）
│  │  ├─ state.local.json        # 本地同步状态（不同步）
│  │  ├─ conflicts.json          # 冲突记录
│  │  └─ trash/                  # 同步删除回收站
│  ├─ ai/                        # AI 相关数据（不同步）
│  │  └─ traces/                 # AI 推理追踪
│  ├─ stats/                     # 写作统计
│  │  ├─ events.local/           # 本地写作事件（不同步）
│  │  └─ cache/                  # 统计缓存（不同步）
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

├─ trash/                        # 已删除文件
└─ sqlite_cache/                 # 可重建缓存（非事实来源）
```
