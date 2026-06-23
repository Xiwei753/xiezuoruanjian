# HarmonyOS NEXT 技术路线

Status: active
Last verified: 2026-06-23
Truth source: docs/TECHNICAL_ROUTE.md

---

## 一、定位

**Harmony 当前为 WIP shell，不认为已完成 Rust Core 全量接入，不认为已完成多端 UI 对齐，不作为正式端发布。**

鸿蒙端是 writer_core 的第三个客户端壳层，遵循 Core-first 架构约束。当前仅完成壳层结构搭建，未接入 Rust Core，所有数据均为 mock。

**核心原则**：
- 鸿蒙端只负责 UI 渲染、ArkUI 生命周期和系统权限
- 业务逻辑全部委托给 Rust Core
- 不自行实现任何业务规则或数据校验

---

## 二、技术栈

| 层级 | 技术选型 | 说明 |
|------|---------|------|
| UI | ArkUI (ArkTS) | HarmonyOS NEXT 原生声明式 UI |
| 语言 | ArkTS | 基于 TypeScript 的鸿蒙应用开发语言 |
| 桥接 | C-ABI / NAPI (未来) | 当前阶段使用 mock bridge |
| 核心 | writer_core (Rust) | 唯一业务事实来源 |

---

## 三、分阶段路线

### 阶段 1：壳层搭建 (当前)

**目标**：建立 ArkTS 页面结构和 mock bridge，验证路由和状态流。

> **约束声明**：当前只是壳层验证，不代表功能完成。所有页面和桥接均使用 mock 数据，未接入 Rust Core，不认为已完成 Rust Core 全量接入，不认为已完成多端 UI 对齐，不作为正式端发布。

**交付物**：
- `apps/harmony/` 目录结构
- Index.ets - 应用入口
- WorkspacePage.ets - 工作区页面
- WritingPage.ets - 写作页面
- StarMapPage.ets - 星图页面
- WriterCoreBridge.ets - Mock 桥接层
- CoreDtos.ets - 数据传输对象定义

**约束**：
- 不接入 Rust Core
- 不使用 UniFFI / NAPI / C++ 桥
- 返回 mock 数据（仅供开发验证，不是正式功能）
- 只验证页面结构和接口形状

### 阶段 2：接口设计 (Desktop 抢修完成后)

**目标**：按 WriterCoreApi 设计 DTO，定义完整接口契约。

**交付物**：
- 完整的 DTO 类型定义
- Bridge 接口头文件
- Harmony 端需要的 Core action 清单

### 阶段 3：真实桥接 (条件成熟后)

**目标**：接入 Rust Core，实现真实业务逻辑。

**可选方案**：
1. **C-ABI + NAPI**：Rust Core 导出 C ABI，通过 NAPI 桥接到 ArkTS
2. **本地服务**：Rust Core 作为本地服务运行，通过 IPC 通信
3. **UniFFI 扩展**：如果 UniFFI 支持鸿蒙，使用统一绑定

**前置条件**：
- Desktop 抢修完成
- Rust Core 接口稳定
- 鸿蒙 NAPI / native 开发工具链成熟

---

## 四、目录结构

```
apps/harmony/
  AppScope/                        应用级配置
    app.json5
  entry/src/main/ets/
    pages/
      Index.ets                   应用入口页面
      WorkspacePage.ets           工作区页面
      WritingPage.ets             写作页面
      StarMapPage.ets             星图页面
      SettingsPage.ets            设置页面
      MainWorkspace.ets           主工作区页面
    bridge/
      IWriterCoreBridge.ets       桥接接口
      MockWriterCoreBridge.ets    Mock 实现
      NativeWriterCoreBridge.ets  Native 空壳
      WriterCoreBridge.ets        桥接入口
    model/
      CoreDtos.ets                数据传输对象
    system/
      HarmonyLifecycle.ets        生命周期管理
      HarmonyStyleAdapter.ets     样式适配器
      HarmonyThemeAdapter.ets     主题适配器
    common/
      AppContext.ets              应用上下文
      AdaptiveContext.ets         自适应上下文
      LayoutPolicyHelper.ets      布局策略辅助
      ScreenPolicyBridge.ets      屏幕策略桥接
    utils/
      MessageKeyMapper.ets        消息键映射
  README.md                       鸿蒙端说明
```

---

## 五、与现有端的关系

| 维度 | Android | Desktop | Harmony |
|------|---------|---------|---------|
| UI 框架 | XML/View | Qt/QML | ArkUI |
| 语言 | Kotlin | Rust + QML | ArkTS |
| 桥接 | UniFFI / JNI | qmetaobject | C-ABI/NAPI (未来) |
| 业务逻辑 | writer_core | writer_core | writer_core |
| 状态 | 稳定 | 稳定 | 壳层搭建 |

---

## 六、风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| ArkTS 生态不成熟 | 阶段 1 只做 mock，不依赖真实桥接 |
| NAPI 工具链不稳定 | 等待 Desktop 稳定后再接入 |
| 鸿蒙 API 变动 | 保持壳层轻量，减少耦合 |

---

## 七、WIP 声明

```
Harmony 当前为 WIP shell
不认为已完成 Rust Core 全量接入
不认为已完成多端 UI 对齐
不作为正式端发布
```

---

## 八、参考文档
- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md) - 跨平台能力契约（已归档）

- [API_CONTRACTS.md](archive/API_CONTRACTS.md) - 接口边界与交互契约（已归档）
- [TECHNICAL_ROUTE.md](TECHNICAL_ROUTE.md) - 全局技术路线
