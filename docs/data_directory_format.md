# 数据目录格式定义

Status: active
Last verified: 2026-08-08
Truth source: protocol
Supersedes: None

数据目录格式定义了存储写作文件、元数据、设置和缓存的精确目录结构和文件格式。它是共享 Rust 核心和所有原生客户端的唯一事实来源。

## 目录由谁决定

Core 不再创建、验证或记忆一个全局 workspace（Issue #600）。各平台自行确定数据根目录，通过 `PlatformInit` 注入 Core。Core 只接收平台提供的路径，不再假设这些目录在平台上的物理名字。

协议定义四个由平台注入的根目录：

| 根目录 | 含义 | 谁提供 |
|--------|------|--------|
| `app_data_root` | 平台应用数据根（全局元数据、设置、统计、星图、最近编辑、同步配置与回收站） | 平台 |
| `projects_root` | 作品根（每个作品一个子目录，各自是独立 Git 仓库） | 平台 |
| `log_dir` | 日志根（应用日志与 crash 记录） | 平台 |
| `cache_dir` | 缓存根（可重建缓存） | 平台 |

Core 只约束这些根目录**内部**的布局；根目录本身放哪里、叫什么名字由平台决定。平台不得要求 Core 拼接或记忆这些根目录。

## Android 示例

```text
/storage/emulated/0/Sujian/      ← app_data_root（稳定 ASCII 磁盘名，Issue #609）
├── projects/                    ← projects_root
├── logs/                        ← log_dir
├── exports/
└── backups/
```

界面显示名（“素笺 / 作品 / 日志 / 导出 / 备份”）走 Android `strings.xml`，
不参与磁盘路径契约；旧版 `/素笺/` 中文目录由 Android 平台在启动时一次性迁移。

## 作品目录（作品级 Git 仓库根）

每个作品目录自身就是一个 Git 仓库（`git init` 在作品创建时完成，旧作品在读取到有效 `project.json` 后自动迁移）。作品的同步以作品目录为根，Git 不包住整个数据根目录；日志、其他作品和应用级配置不会进入某个作品仓库。

```text
<projects_root>/<project_id>/      ← 作品目录 = Git 仓库根
├── .git/
├── project.json                   # 作品元数据
├── volumes/
│  └── <volume_id>/                # 卷目录
│     ├── volume.json              # 卷元数据
│     └── chapters/
│        └── <chapter_id>/         # 章节目录
│           ├── chapter.md         # 正文（纯文本）
│           └── chapter.meta.json  # 章节元数据
├── characters/                    # 角色卡（如适用）
└── app-meta/sync/                 # 该作品自己的同步元数据（不含用户同步 config/secrets）
   ├── state.local.json            # 本地同步状态（不同步）
   ├── conflicts.json              # 冲突记录（不同步）
   └── manifest.sync.json          # 同步清单（参与同步）
```

## 应用数据根目录（应用级 Git 仓库根）

应用级数据全部位于 `app_data_root`。`app_data_root` 自身是一个独立 Git 仓库（应用级仓库），仅同步真正需要跨设备的数据：设置、全局星图、主题调色板。作品正文、日志、导出、备份、设备专属数据、同步凭证、缓存、统计本地状态均不进入应用级仓库。

```text
<app_data_root>/                  ← 应用级 Git 仓库根
├── .git/                          # 应用级仓库元数据
├── settings.local.json            # 设备特定设置（不同步）
├── settings.sync.json             # 跨设备设置（应用级白名单）
├── recent_edits.json              # 最近编辑记录（不同步）
├── device/
│  └── current_device.json         # 设备信息（不同步）
├── app-meta/
│  ├── sync/                       # 应用级同步元数据
│  │  ├── config.local.json        # 应用级同步配置（不同步）
│  │  ├── state.local.json         # 本地同步状态（不同步）
│  │  ├── conflicts.json           # 冲突记录（不同步）
│  │  └── manifest.sync.json       # 同步清单（不同步）
│  ├── stats/
│  │  ├── events.local/            # 本地写作事件（不同步）
│  │  └── daily/                   # 统计日报（不同步）
│  └── transactions/               # 写入事务记录（不同步）
├── themes/palettes/               # 按设备与指纹保存的调色板记录（应用级白名单）
├── starmaps/                      # 星图数据（应用级白名单）
```

### 应用级同步白名单/黑名单

同步根 = `app_data_root`，路径相对应用数据根：

- **白名单（参与同步）**：`settings.sync.json`、`starmaps/**`、`themes/palettes/**`。
- **黑名单（绝不参与）**：`projects/`（projects_root）、`logs/`（log_dir）、`exports/`、`backups/`、`settings.local.json`、`recent_edits.json`、含 `secret` 的路径（`app-meta/sync/*secret*`）、`device/`、`app-meta/stats/`、`app-meta/transactions/`、`.git/`、`.tmp`/`.lock` 后缀、含 `cache`/`tmp`/`backups`/`sqlite_cache` 的路径。

`log_dir` 由平台提供，与 `app_data_root` 相互独立（Android 为 `Sujian/logs/`），不位于数据根目录下。

## 同步目标与远端仓库组织

系统只有一个全局 `SyncProfile`、一个远端 Git 仓库、一次全量同步（Issue #630）。Core 内部按 `SyncScope::App / Project` 区分两个 target，只用于路径过滤和远端前缀路由，不再暴露成两套用户配置。

本地目录结构不变：`app_data_root` 继续是应用数据根，每个 `project_root` 继续是独立作品目录与独立本地 Git 仓库。同步引擎把不同本地根映射到同一个远端仓库的不同前缀。

远端仓库组织：

```text
app/
  settings.sync.json
  starmaps/**
  themes/palettes/**
  app-meta/sync/manifest.sync.json
projects/<project_id>/
  project.json
  volumes/**
  characters/**
  app-meta/sync/manifest.sync.json
```

| 内部 target | 本地同步根 | 远端前缀 | 白名单 | 同步内容 |
|-------------|-----------|----------|--------|----------|
| **App** | `app_data_root` | `app/` | `settings.sync.json`/`starmaps/**`/`themes/palettes/**` | 设置、全局星图、主题调色板 |
| **Project** | `project_root`（`projects_root/<project_id>`） | `projects/<project_id>/` | `project.json`/`volumes/**`/`characters/**`/`app-meta/sync/manifest.sync.json` | 单部作品正文、元数据、作品自己的同步状态 |

全局同步配置只存在一份：`<app_data_root>/app-meta/sync/config.local.json`。作品目录下不再保存用户同步 config/secrets，只保留该 target 的本地 `state.local.json / manifest.sync.json / conflicts.json`。
