# HarmonyOS NEXT 客户端

> **[WIP] 本端当前为工作进展中（Work In Progress）的壳层状态，不具备正式可用功能。**
>
> - 不认为已完成 Rust Core 全量接入
> - 不认为已完成多端 UI 对齐
> - 不作为正式端发布
> - Mock 模式仅供开发验证，不是正式功能

## 概述

这是写作软件的 HarmonyOS NEXT 客户端壳层，遵循 Core-first 架构约束，所有业务逻辑委托给 Rust Core。

## 当前状态

**阶段 1：壳层搭建 - 已搭建（WIP）**

- 已完成 DevEco Studio 工程骨架
- 已完成 ArkTS 页面结构
- 已完成 Mock Bridge 实现
- 已完成 接口与实现分离
- 已完成 系统服务接口定义
- 已完成 主题适配器
- 已完成 生命周期管理
- 已完成 路由和状态流验证

> 注意：以上仅代表壳层结构已搭建，不代表功能已完成。当前所有页面和桥接均使用 mock 数据，未接入 Rust Core。

**阶段 2：接口设计 - 待推进**

- 已整理 Core Action Map 文档
- 已整理 Native Bridge 接入计划
- 已整理 DTO 类型定义
- 已整理 ResultEnvelope 统一返回格式
- 等待 Core 接口稳定后方可推进

**阶段 3：真实桥接 - 待开始**

- 待接入 Rust Core
- 待 C-ABI/NAPI 方案实施

---

**WIP 声明**：

Harmony 当前为 WIP shell。不认为已完成 Rust Core 全量接入，不认为已完成多端 UI 对齐，不作为正式端发布。

## 目录结构

```
apps/harmony/
  AppScope.ets                    应用级配置
  oh-package.json5                依赖配置
  build-profile.json5             构建配置
  hvigorfile.ts                   构建脚本
  entry/
    build-profile.json5           Entry 模块构建配置
    hvigorfile.ts                 Entry 构建脚本
    src/main/
      module.json5                模块配置
      resources/
        base/
          element/
            string.json           字符串资源
            color.json            颜色资源
          media/                  媒体资源
          profile/
            main_pages.json       页面路由配置
        rawfile/                  原始文件
      ets/
        entryability/
          EntryAbility.ets        应用入口
        pages/
          Index.ets               首页
          WorkspacePage.ets       工作区页面
          WritingPage.ets         写作页面
          StarMapPage.ets         星图页面
          SettingsPage.ets        设置页面
        bridge/
          IWriterCoreBridge.ets   桥接接口
          MockWriterCoreBridge.ets Mock 实现
          NativeWriterCoreBridge.ets Native 空壳
        model/
          CoreDtos.ets            数据传输对象
        system/
          HarmonyFileAccess.ets   文件访问接口
          HarmonySecureStorage.ets 安全存储接口
          HarmonyNetworkState.ets 网络状态接口
          HarmonyLifecycle.ets    生命周期管理
          HarmonyThemeAdapter.ets 主题适配器
        common/
          AppContext.ets          应用上下文
  README.md                       本文件
```

## 核心模块

### Bridge 架构

```
IWriterCoreBridge (接口)
├── MockWriterCoreBridge (Mock 实现)
└── NativeWriterCoreBridge (Native 空壳，待实现)
```

### 系统服务（对齐 HarmonyOS NEXT API 12）

- **HarmonyFileAccess**: 文件系统访问 → 包装 `@ohos.file.fs`
- **HarmonySecureStorage**: KV 存储 → 包装 `@ohos.data.preferences`
- **HarmonyNetworkState**: 网络状态 → 包装 `@ohos.net.connection`
- **HarmonyLifecycle**: 生命周期 → 对齐 UIAbility 回调
- **HarmonyThemeAdapter**: 主题适配（内部逻辑，不依赖系统 API）

### 应用上下文

AppContext 统一管理所有服务实例，支持 Mock/Native 环境切换。

## 页面说明

### Index.ets - 首页
- 工作区概览
- 写作统计卡片
- 最近编辑列表
- 作品列表入口
- 快捷操作网格

### WorkspacePage.ets - 工作区页面
- 作品详情
- 卷和章节树形结构
- 创建/删除卷和章节
- 进入写作页面

### WritingPage.ets - 写作页面
- 文本编辑器
- 字数统计
- 自动保存
- 写作会话跟踪
- 设置集成

### StarMapPage.ets - 星图页面
- 星图可视化
- 节点和边渲染
- 节点信息面板
- 缩放控制
- 图例显示
- 星图切换

### SettingsPage.ets - 设置页面
- 编辑器设置（字号、行高、自动保存）
- 显示设置（字数统计、行号、换行）
- 主题设置（深色/浅色/跟随系统）
- 同步设置（配置、诊断、执行）

## Bridge 接口

IWriterCoreBridge 定义了完整的接口：

- **Workspace**: listWorkspaces, openWorkspace, validateWorkspace
- **Project**: listProjects, getProjectTree, createProject, renameProject, deleteProject
- **Volume**: listVolumes, createVolume, renameVolume, deleteVolume, reorderVolumes
- **Chapter**: listChapters, loadChapter, saveChapter, clearChapter, createChapter, renameChapter, deleteChapter
- **StarMap**: listStarMaps, getStarMapGraph, createStarMap, deleteStarMap, renameStarMap
- **Settings**: getLocalSettings, saveLocalSettings, getSyncableSettings, saveSyncableSettings
- **Sync**: loadSyncConfig, saveSyncConfig, syncDryRun, syncDiagnostics, performSync
- **Stats**: getWritingStats, calculateWordCount, processWritingEvent

## 开发计划

### 阶段 2：接口设计
- 已整理 Core Action Map 文档
- 已整理 Native Bridge 接入计划
- 已整理 DTO 类型定义
- 已整理 ResultEnvelope 统一返回格式

### 阶段 3：真实桥接
- C-ABI + NAPI 方案评估
- 编译 Rust Core 为 HarmonyOS 目标
- 实现 NativeWriterCoreBridge
- 集成测试

## 构建与运行

### 前置条件
- DevEco Studio 5.0+
- HarmonyOS NEXT SDK
- Node.js 16+

### 构建步骤
1. 使用 DevEco Studio 打开 `apps/harmony/` 目录
2. 等待依赖安装完成
3. 选择模拟器或真机
4. 点击运行
### Mock 模式

当前所有数据都是 Mock 实现，无需配置 Rust Core。

> **重要**：Mock 模式仅供开发验证页面结构和接口形状，不是正式功能。Mock 数据不代表真实业务逻辑，不应用于功能测试或用户体验评估。

## 云端构建（CI/CD）

### Workflow 配置

新增 `.github/workflows/harmony_build.yml`，用于自动化构建 HarmonyOS HAP 包。

- **触发条件**：push 到 `apps/harmony/**` 路径，或手动触发（workflow_dispatch）
- **构建环境**：`ghcr.io/sanchuanhehe/harmony-next-pipeline-docker/harmonyos-ci-image:latest`
- **构建步骤**：
  1. `ohpm install --all` — 安装依赖
  2. `hvigorw assembleHap` — 构建 HAP
  3. 上传构建产物为 GitHub Artifact

> **注意**：第一阶段只构建无签名/调试 HAP，不代表正式发布包。

## 签名规则

### 签名材料管理

- 签名材料（`.p12`、`.cer`、`.p7b`、密码等）**不得提交到仓库**
- `build-profile.json5` 中的签名配置已清空为占位符，仅用于本地开发参考
- **旧 debug 签名材料已废弃**：此前曾误提交的 DevEco Studio 自动生成的 debug 签名（含加密密码、本地路径）已从仓库清除。该签名仅用于开发调试，不具备任何发布效力。如曾使用该签名，请重新生成新的 debug signing material。
- 任何密码解密脚本（如 `DecryptDevEcoPassword.java`、`decrypt_pwd.py`）不得提交到仓库，已删除

### GitHub Secrets 注入

后续签名通过 GitHub Secrets 注入，所需 Secret 列表：

| Secret 名称 | 说明 |
|-------------|------|
| `HARMONY_P12_BASE64` | .p12 证书文件（Base64 编码） |
| `HARMONY_CERT_BASE64` | .cer 证书文件（Base64 编码） |
| `HARMONY_PROFILE_BASE64` | .p7b 描述文件（Base64 编码） |
| `HARMONY_KEY_ALIAS` | 密钥别名 |
| `HARMONY_KEY_PASSWORD` | 密钥密码 |
| `HARMONY_STORE_PASSWORD` | 密钥库密码 |

### 文件转 Base64 命令（PowerShell）

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("sujian.p12")) | Set-Content p12.txt
[Convert]::ToBase64String([IO.File]::ReadAllBytes("sujian.cer")) | Set-Content cert.txt
[Convert]::ToBase64String([IO.File]::ReadAllBytes("sujian.p7b")) | Set-Content profile.txt
```

## 重要声明

- 云端构建 artifact 仅用于开发测试，**不代表正式发布包**
- 签名材料**不得提交到仓库**
- 第一阶段只构建无签名/调试 HAP

## 注意事项

1. 当前阶段使用 mock 数据，不涉及真实文件系统，mock 仅供开发验证
2. 所有业务逻辑最终由 Rust Core 处理，当前尚未接入
3. 鸿蒙端只负责 UI 渲染和系统交互
4. 不自行实现任何业务规则或数据校验
5. 遵循 Core-first 架构约束
6. 本端为 WIP 壳层，不作为正式端发布

## 参考文档

- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](../../docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md)
- [API_CONTRACTS.md](../../docs/API_CONTRACTS.md)
- [harmony_route.md](../../docs/harmony_route.md)
- [harmony_bridge_contract.md](../../docs/harmony_bridge_contract.md)
- [harmony_core_action_map.md](../../docs/harmony_core_action_map.md)
- [harmony_native_bridge_plan.md](../../docs/harmony_native_bridge_plan.md)
