# HarmonyOS NEXT 客户端

## 概述

这是写作软件的 HarmonyOS NEXT 客户端壳层，遵循 Core-first 架构约束，所有业务逻辑委托给 Rust Core。

## 当前状态

**阶段 1：壳层搭建**

- ✅ ArkTS 页面结构
- ✅ Mock Bridge 实现
- ✅ 路由和状态流验证
- ⏳ 等待接入真实 Rust Core

## 目录结构

```
apps/harmony/
  AppScope.ets                    应用级配置
  entry/src/main/ets/
    pages/
      Index.ets                   应用入口页面
      WorkspacePage.ets           工作区页面
      WritingPage.ets             写作页面
      StarMapPage.ets             星图页面
    bridge/
      WriterCoreBridge.ets        桥接层（当前为 mock）
    model/
      CoreDtos.ets                数据传输对象
  README.md                       本文件
```

## 页面说明

### Index.ets
- 应用入口页面
- 显示工作区概览
- 最近编辑列表
- 作品列表入口
- 星图入口

### WorkspacePage.ets
- 作品详情页面
- 卷和章节的树形结构
- 新建卷和章节
- 进入写作页面

### WritingPage.ets
- 写作编辑器页面
- 文本编辑区域
- 字数统计
- 自动保存功能
- 设置集成（字号、行距、字体）

### StarMapPage.ets
- 星图可视化页面
- 节点和边的渲染
- 节点信息面板
- 缩放控制
- 图例显示

## Bridge 接口

WriterCoreBridge 当前返回 mock 数据，接口形状与 Rust Core Capability API 对齐：

- `listWorkspaces()` - 列出工作区
- `openWorkspace(path)` - 打开工作区
- `listProjects()` - 列出项目
- `createProject(name)` - 创建项目
- `deleteProject(id)` - 删除项目
- `listVolumes(projectId)` - 列出卷
- `createVolume(projectId, name)` - 创建卷
- `listChapters(volumeId)` - 列出章节
- `loadChapter(chapterId)` - 加载章节内容
- `saveChapter(chapterId, text)` - 保存章节内容
- `createChapter(volumeId, name)` - 创建章节
- `deleteChapter(chapterId)` - 删除章节
- `listStarMaps()` - 列出星图
- `getStarMapGraph(starmapId)` - 获取星图图数据
- `getLocalSettings()` - 获取本地设置
- `saveLocalSettings(settings)` - 保存本地设置
- `getWritingStats()` - 获取写作统计
- `calculateWordCount(text)` - 计算字数

## 开发计划

### 阶段 2：接口设计
- 完整的 DTO 类型定义
- Bridge 接口头文件
- Harmony 端需要的 Core action 清单

### 阶段 3：真实桥接
- C-ABI + NAPI 方案评估
- 或本地服务方案评估
- 接入 Rust Core

## 注意事项

1. 当前阶段使用 mock 数据，不涉及真实文件系统
2. 所有业务逻辑最终由 Rust Core 处理
3. 鸿蒙端只负责 UI 渲染和系统交互
4. 不自行实现任何业务规则或数据校验

## 参考文档

- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](../../docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md)
- [API_CONTRACTS.md](../../docs/API_CONTRACTS.md)
- [harmony_route.md](../../docs/harmony_route.md)
- [harmony_bridge_contract.md](../../docs/harmony_bridge_contract.md)
