# HarmonyOS NEXT 客户端

## 概述

这是写作软件的 HarmonyOS NEXT 客户端壳层，遵循 Core-first 架构约束，所有业务逻辑委托给 Rust Core。

## 当前状态

**阶段 1：壳层搭建 - 已完成**

- ✅ DevEco Studio 工程骨架
- ✅ ArkTS 页面结构
- ✅ Mock Bridge 实现
- ✅ 接口与实现分离
- ✅ 系统服务接口定义
- ✅ 主题适配器
- ✅ 生命周期管理
- ✅ 路由和状态流验证

**阶段 2：接口设计 - 进行中**

- ✅ Core Action Map 文档
- ✅ Native Bridge 接入计划
- ✅ 完整的 DTO 类型定义
- ✅ ResultEnvelope 统一返回格式
- ⏳ 等待 Core 接口稳定

**阶段 3：真实桥接 - 待开始**

- ⏳ 接入 Rust Core
- ⏳ C-ABI/NAPI 方案实施

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

### 系统服务

- **HarmonyFileAccess**: 文件系统访问接口
- **HarmonySecureStorage**: 安全存储接口（Token、密钥）
- **HarmonyNetworkState**: 网络状态监控
- **HarmonyLifecycle**: 应用生命周期管理
- **HarmonyThemeAdapter**: 主题适配（深色/浅色模式）

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
- ✅ Core Action Map 文档
- ✅ Native Bridge 接入计划
- ✅ 完整的 DTO 类型定义
- ✅ ResultEnvelope 统一返回格式

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

## 注意事项

1. 当前阶段使用 mock 数据，不涉及真实文件系统
2. 所有业务逻辑最终由 Rust Core 处理
3. 鸿蒙端只负责 UI 渲染和系统交互
4. 不自行实现任何业务规则或数据校验
5. 遵循 Core-first 架构约束

## 参考文档

- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](../../docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md)
- [API_CONTRACTS.md](../../docs/API_CONTRACTS.md)
- [harmony_route.md](../../docs/harmony_route.md)
- [harmony_bridge_contract.md](../../docs/harmony_bridge_contract.md)
- [harmony_core_action_map.md](../../docs/harmony_core_action_map.md)
- [harmony_native_bridge_plan.md](../../docs/harmony_native_bridge_plan.md)
