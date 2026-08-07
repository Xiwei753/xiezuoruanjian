# 数据目录格式定义

Status: active
Last verified: 2026-08-08
Truth source: protocol
Supersedes: None

数据目录格式定义了存储写作文件、元数据、设置和缓存的精确目录结构和文件格式。它是共享 Rust 核心和所有原生客户端的唯一事实来源。

各平台自行管理数据根目录（如 Android 的 `/storage/emulated/0/素笺/`、桌面端的用户配置目录下 `素笺/`）。数据根目录下直接划分作品、日志、导出、备份等子目录，不再有统一的 workspace 概念或 workspace_manifest.json。

```
<app_data_root>/                # 平台数据根目录（各平台自行决定路径）
├─ app-meta/                     # 全局元数据和设置
│  ├─ settings/
│  │  ├─ settings.local.json     # 设备特定设置（不同步）
│  │  └─ settings.sync.json      # 跨设备设置（同步）
│  ├─ sync/                      # 同步状态与配置
│  │  ├─ sync_config.json        # 同步配置（enabled、backend_type、remote_url、transport、branch、auto_sync）
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
│  ├─ themes/                    # 主题数据
│  │  └─ palettes/               # 按设备与指纹保存的调色板记录
│  └─ logs/                      # 应用日志
├─ projects/                     # 作品目录（即“素笺/作品/”）
│  └─ <project_id>/              # 单部作品目录
│     ├─ project.json            # 作品元数据
│     ├─ volumes/
│     │  └─ <volume_id>/         # 卷目录
│     │     ├─ volume.json       # 卷元数据
│     │     └─ chapters/
│     │        └─ <chapter_id>/  # 章节目录
│     │           ├─ chapter.md  # 正文
│     │           └─ chapter.meta.json # 章节元数据
│     ├─ characters/             # 角色卡（如适用）
│     ├─ starmap/                # 该作品的星图数据
│     └─ .git/                   # 该作品的 Git 仓库（同步与版本管理）
├─ trash/                        # 已删除文件
└─ sqlite_cache/                 # 可重建缓存（非事实来源）
```
