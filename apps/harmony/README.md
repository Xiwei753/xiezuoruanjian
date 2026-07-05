# HarmonyOS NEXT 客户端

## 概述

这是写作软件的 HarmonyOS NEXT 客户端，遵循 Core-first 架构约束，所有业务逻辑委托给 Rust Core。默认通过 NAPI 调用 Rust Core，不自动降级 mock。

## 当前状态

**Native 桥接已实现，默认走 Rust Core。**

- ArkTS 通过 `import nativeModule from 'libwriter_core.so'` 调用 NAPI
- C++ NAPI 层（`napi_init.cpp`）注册了约 50 个 `nativeXxx` 函数
- C++ 链接 `libwriter_core_ffi.so`（预编译 Rust FFI 库）
- `CMakeLists.txt` 检查 `prebuilt/${OHOS_ARCH}/libwriter_core_ffi.so`，不存在则 fatal error
- `writer_core_bridge.h` 声明了所有 C-ABI 函数
- Rust Core 通过 C-ABI 导出 `writer_core_init`、workspace、project、chapter、settings、sync、stats、starmap 等接口
- `MockWriterCoreBridge` 仍然存在，但仅用于开发调试，不是默认路径

## 架构说明

四层桥接架构：

```
Rust Core (libwriter_core_ffi.so)
  ↓ C-ABI (writer_core_bridge.h)
C++ NAPI 层 (napi_init.cpp, libwriter_core.so)
  ↓ NAPI
ArkTS NativeWriterCoreBridge
  ↓
页面 / ViewModel
```

1. **Rust FFI 层**：`libwriter_core_ffi.so`，通过 C-ABI 导出 `writer_core_init`、workspace、project、chapter、settings、sync、stats、starmap 等函数
2. **C Header 层**：`writer_core_bridge.h`，声明所有 C-ABI 函数签名
3. **NAPI C++ 层**：`napi_init.cpp`，将 C-ABI 函数包装为约 50 个 `nativeXxx` NAPI 函数，注册到 `libwriter_core.so`
4. **ArkTS 层**：`NativeWriterCoreBridge`，通过 `import nativeModule from 'libwriter_core.so'` 调用 NAPI 函数

## 目录结构

```
apps/harmony/
  AppScope/                        应用级配置
    app.json5
  oh-package.json5                依赖配置
  build-profile.json5             构建配置
  hvigorfile.ts                   构建脚本
  entry/
    build-profile.json5           Entry 模块构建配置
    hvigorfile.ts                 Entry 构建脚本
    src/main/
      module.json5                模块配置
      cpp/                        NAPI C++ 桥接层
        CMakeLists.txt
        napi_init.cpp
        writer_core_bridge.h
        libs/                     预编译 Rust FFI 库
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
          MainWorkspace.ets       主工作区页面
          WorkspacePage.ets       工作区页面
          WritingPage.ets         写作页面
          StarMapPage.ets         星图页面
          SettingsPage.ets        设置页面
        bridge/
          IWriterCoreBridge.ets   桥接接口
          MockWriterCoreBridge.ets Mock 实现（仅开发调试用）
          NativeWriterCoreBridge.ets Native 实现（默认）
          WriterCoreBridge.ets    桥接入口
        model/
          CoreDtos.ets            数据传输对象
        system/
          HarmonyLifecycle.ets    生命周期管理
          HarmonyStyleAdapter.ets 样式适配器
          HarmonyThemeAdapter.ets 主题适配器
        common/
          AppContext.ets          应用上下文
          AdaptiveContext.ets     自适应上下文
          LayoutPolicyHelper.ets  布局策略辅助
          ScreenPolicyBridge.ets  屏幕策略桥接
        utils/
          MessageKeyMapper.ets    消息键映射
  README.md                       本文件
```

## 核心模块

### Bridge 架构

```
IWriterCoreBridge (接口)
├── MockWriterCoreBridge (仅开发调试用)
└── NativeWriterCoreBridge (默认，已实现)
```

- **NativeWriterCoreBridge**：默认桥接，通过 NAPI 调用 Rust Core。初始化失败时显示错误，不降级 mock。
- **MockWriterCoreBridge**：仅用于开发调试，不作为默认路径。如需使用需手动切换环境配置。

### 系统服务（对齐 HarmonyOS NEXT API 12）

- **HarmonyLifecycle**: 生命周期 → 对齐 UIAbility 回调
- **HarmonyStyleAdapter**: 样式适配（内部逻辑，不依赖系统 API）
- **HarmonyThemeAdapter**: 主题适配（内部逻辑，不依赖系统 API）

> 注：HarmonyFileAccess、HarmonySecureStorage、HarmonyNetworkState 等系统服务接口尚未实现为独立文件，相关能力暂通过 NativeWriterCoreBridge 和 MockWriterCoreBridge 内部调用系统 API 实现。

### 应用上下文

AppContext 统一管理所有服务实例，默认 `environment=native`。

## 初始化流程

1. `EntryAbility.setAbilityContext` → 设置应用上下文
2. `Index.initAndLoad` → 调用 `NativeWriterCoreBridge.initialize(workspacePath)`
3. 初始化成功 → 加载工作区数据
4. 初始化失败 → 显示错误信息，不降级 mock

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

## 构建与运行

### 前置条件
- DevEco Studio 5.0+
- HarmonyOS NEXT SDK
- Node.js 16+
- 预编译 `libwriter_core_ffi.so` 放置于 `entry/src/main/prebuilt/${OHOS_ARCH}/`

### 构建步骤
1. 使用 DevEco Studio 打开 `apps/harmony/` 目录
2. 等待依赖安装完成
3. 选择模拟器或真机
4. 点击运行

> Mock 模式仅用于开发调试，不是默认路径。如需使用 Mock 模式，需手动将 AppContext 的 environment 切换为 mock。

## 云端构建（CI/CD）

> **注意**：HarmonyOS CI/CD workflow 尚未配置，待后续推进。

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

## 当前限制

- 不要求编译 HAP
- 不要求 DevEco 实测
- 不要求真机验证
- 仅要求仓库代码、类型、单元测试、静态检查通过

## 注意事项

1. 鸿蒙端只负责 UI 渲染和系统交互
2. 不自行实现任何业务规则或数据校验
3. 遵循 Core-first 架构约束
4. Mock 模式仅用于开发调试，不是默认路径

## 参考文档

- [技术路线与架构约束](../../docs/TECHNICAL_ROUTE.md)
- [工作区格式](../../docs/workspace_format.md)
- [同步规则](../../docs/sync_rules.md)
