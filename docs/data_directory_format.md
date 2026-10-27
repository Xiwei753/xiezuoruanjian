# 数据目录格式定义

Status: active
Last verified: 2026-10-18
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

应用级数据全部位于 `app_data_root`。`app_data_root` 自身是一个独立 Git 仓库（应用级仓库），仅同步真正需要跨设备的数据：设置、全局星图，以及按来源设备分区的写作统计事件（见「统计设备组」）。作品正文、日志、导出、备份、设备专属数据（包括 Android 设备配色）、同步凭证、缓存、旧版统计本地状态均不进入应用级仓库。

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
│  │  ├── full_state.local.json    # 全量同步总体状态（不同步）
│  │  ├── last_report.local.json   # 最近一次按数据组记录的同步报告（不同步）
│  │  ├── manifest.sync.json       # 旧 App 同步清单（迁移期间保留）
│  │  └── groups/                  # 按 App 数据组隔离的本地同步元数据
│  │     └── <group-key>/
│  │        ├── manifest.sync.json
│  │        ├── state.local.json
│  │        └── conflicts.json
│  ├── stats/
│  │  ├── events/                  # 按来源设备分区的写作事件（经统计设备组同步）
│  │  │  └── <device_id>/
│  │  │     ├── YYYY-MM-DD.events.jsonl
│  │  │     ├── YYYY-MM-DD.sessions.jsonl
│  │  │     └── proof/
│  │  │        ├── <chapter_id>.checkpoints.jsonl
│  │  │        └── archive/YYYY-MM-DD.events.jsonl.zst
│  │  ├── events.local/            # 旧版本地写作事件（迁移后只读保留，不同步）
│  │  ├── daily/                   # 统计日报（不同步）
│  │  ├── events.device_partitions.v1  # 事件库迁移完成标记（不同步）
│  │  └── event_conflicts.local.jsonl  # 跨分区重复 event_id 诊断（不同步）
│  └── transactions/               # 写入事务记录（不同步）
├── local-theme-palettes/          # Android 本地设备配色（绝不参与同步）
│  └── <device_id>/<fingerprint>.json
├── themes/palettes/               # 旧版调色板目录（只作 Android 升级迁移输入，不同步）
├── starmaps/                      # 星图数据（应用级白名单）
```

### 应用级同步白名单/黑名单

同步根 = `app_data_root`，路径相对应用数据根：

- **白名单（参与同步）**：`settings.sync.json`、`starmaps/**`、`stats/events/<device_id>/YYYY-MM-DD.events.jsonl`、`stats/events/<device_id>/YYYY-MM-DD.sessions.jsonl`、`stats/events/<device_id>/proof/**`（仅通过对应设备数据组同步，本机只写自己的分区）。`proof/` 包含章节检查点与封口后的压缩事件段，同样按来源设备独占写入。
- **Android 设备配色**：`local-theme-palettes/**` 只在当前 Android 设备本地使用；旧版 `themes/palettes/**` 仅作升级迁移输入，二者均不参与 Git 或近场同步。旧远端 `app/themes/palettes/**` 由新客户端忽略，不产生删除 tombstone；Qt、HarmonyOS 与 Windows 不读取、不展示、不应用这些配色。
- **黑名单（绝不参与）**：`projects/`（projects_root）、`logs/`（log_dir）、`exports/`、`backups/`、`settings.local.json`、`recent_edits.json`、含 `secret` 的路径（`app-meta/sync/*secret*`）、`device/`、`app-meta/stats/`（旧版单机事件库、迁移标记与冲突诊断文件）、`app-meta/transactions/`、`.git/`、`.tmp`/`.lock` 后缀、含 `cache`/`tmp`/`backups`/`sqlite_cache` 的路径。

### 统计设备组

写作统计事件按来源设备独立成组同步（详见 `docs/sync_rules.md`「统计设备组」）：

- 事件目录：`stats/events/<device_id>/YYYY-MM-DD.events.jsonl`，文件按 UTC 日命名，事件内 `local_date` 仍是事件发生地的业务日，两个日期概念不混用。
- 编辑证明：同一事件行可选携带 `proof`（`formatVersion`、设备内持久递增 `seq`、`sessionId`、`baselineId`、`cause`、实际 `changes`、前后选区、before/after 全文 SHA-256、命令链摘要、prev/event hash）。旧 JSONL 没有 `proof` 仍按旧字段读取，只可统计、不能逐字回放；不得为旧历史伪造变更列表。一次成功编辑事务只写一条事件，事务内的插入/删除顺序保留；失败、过期 revision、未提交 composition 不产生事件。
- 事件正文摘要：`before_text_sha256` / `after_text_sha256` 是编辑器内核在事务提交前后对**真实完整正文**计算的 SHA-256，和 `before_content_hash` / `after_content_hash`（命令链摘要）分开记录。平台必须随事件传递内核摘要；Core 写盘前核对上一条 `after_text_sha256` 与本次 `before_text_sha256`，不一致说明漏记或旁路修改，不能继续伪造连续链。
- 检查点：`stats/events/<device_id>/proof/<chapter_id>.checkpoints.jsonl`，按章节与设备保存基线/阶段性完整正文、全文 SHA-256、对应事件序号 `seq` 与覆盖的日志位置。检查点只是回放加速索引，事件才是长期事实；同序号不同正文拒绝覆盖。每次保存先用已验证检查点和事件重建同序号正文，摘要一致才写 `save` 检查点；不一致必须先写带来源的外部变更事件（同步覆盖标 `SyncRemote` 并携带来源设备/父事件；无法确认来源才标 `Unknown`），不能借旧 `seq` 盖章。打开回放时会先顺序校验整条章节分支，近端检查点不能掩盖更早事件被篡改；校验失败时所有帧和导出都明确失败。
- 封口归档：`stats/events/<device_id>/proof/archive/YYYY-MM-DD.events.jsonl.zst`。活跃当天只追加 JSONL，封口后才把不可变历史压缩为 zstd 事件段；归档段与其来源设备检查点留在同一设备分区，归档后不再改写。
- 计时目录：同分区 `YYYY-MM-DD.sessions.jsonl`，每行是一条已测量的前台编辑器写作切片（`event_id`、`session_id`、起止毫秒、项目/卷/章、来源本地日、暂停原因）。切片不是两次按键时间差；本机只追加自己的分区并按 `event_id` 幂等去重。
- 组 id `stats_device:<device_id>`，远端 storage key `stats-device-<hex>`；每台设备独占写入自己的分区，下载分区只读，其它设备不重写。跨设备回放按各设备内部的 `seq` 与 before/after hash、基线因果衔接，不按毫秒交错不同设备的事务；同步导入的正文变化显式记为 `SyncRemote`/依赖事件，不计入人工输入，分叉按设备分支展示；回放器按章节内事件位置 0..N 定位，按相邻事件真实时间差和倍速播放，全局 seq 只用于链校验。
- 聚合时 Core 扫描全部分区并按 `event_id` 去重（本机优先），不维护可被 LWW 覆写的每日汇总账本；字数仍从事件投影派生，真实写作秒数只从计时切片派生。旧事件没有计时切片，不得反推或补写时长，也不与新计时样本混算字/时。
- 删除作品后，已写入的事件与检查点作为历史证明保留在设备分区（不随作品目录删除而抹除）；导出证明包包含历史正文，平台必须在导出前明确提示用户。旧 `app-meta/stats/events.local/` 只作为一次性迁移输入，迁移标记写入后不再读写。

`log_dir` 由平台提供，与 `app_data_root` 相互独立（Android 为 `Sujian/logs/`），不位于数据根目录下。

## 同步目标与远端仓库组织

系统只有一个全局 `SyncProfile`、一个远端 Git 仓库、一次全量同步（Issue #630）。Core 内部按 `SyncScope::App / Project` 区分两个 target，只用于路径过滤和远端前缀路由，不再暴露成两套用户配置。

本地目录结构不变：`app_data_root` 继续是应用数据根，每个 `project_root` 继续是独立作品目录与独立本地 Git 仓库。同步引擎把不同本地根映射到同一个远端仓库的不同前缀。

远端仓库组织：

```text
app/
  settings.sync.json
  starmaps/**
  app-meta/sync/app_groups.sync.json    # App 数据组当前 generation 目录清单
  __groups__/<group-key>/
    __generations__/<generation>/**     # 该数据组当前 generation 内容
projects/<project_id>/
  project.json
  volumes/**
  characters/**
  app-meta/sync/manifest.sync.json
```

统计设备组的远端路径为 `app/__groups__/stats-device-<hex>/__generations__/<generation>/stats/events/<device_id>/...`，由对应设备独占写入。

| 内部 target | 本地同步根 | 远端前缀 | 白名单 | 同步内容 |
|-------------|-----------|----------|--------|----------|
| **App** | `app_data_root` | `app/` | `settings.sync.json`/`starmaps/**`/`stats/events/<device_id>/**` | 设置、全局星图、写作统计事件分区 |
| **Project** | `project_root`（`projects_root/<project_id>`） | `projects/<project_id>/` | `project.json`/`volumes/**`/`characters/**`/`app-meta/sync/manifest.sync.json` | 单部作品正文、元数据、作品自己的同步状态 |

全局同步配置只存在一份：`<app_data_root>/app-meta/sync/config.local.json`。作品目录下不再保存用户同步 config/secrets，只保留该 target 的本地 `state.local.json / manifest.sync.json / conflicts.json`。
