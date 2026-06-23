# HarmonyOS Bridge 契约

Status: draft
Last verified: 2026-06-14
Truth source: docs/TECHNICAL_ROUTE.md

---

## 一、概述

本文档定义 HarmonyOS 端与 Rust Core 之间的桥接契约。当前阶段使用 mock 实现，后续将对接真实 Core。

---

## 二、桥接架构

```
┌─────────────────────────────────────┐
│         ArkUI Pages                 │
│  (Index, Workspace, Writing, StarMap)│
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│       WriterCoreBridge              │
│  (当前: Mock / 未来: C-ABI/NAPI)    │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│        writer_core (Rust)           │
│    (唯一业务事实来源)                │
└─────────────────────────────────────┘
```

---

## 三、接口清单

### 1. Workspace 相关

```typescript
// 列出所有工作区
listWorkspaces(): Promise<Workspace[]>

// 打开工作区
openWorkspace(path: string): Promise<WorkspaceState>
```

### 2. Project 相关

```typescript
// 列出项目
listProjects(): Promise<Project[]>

// 创建项目
createProject(name: string): Promise<Project>

// 删除项目
deleteProject(projectId: string): Promise<boolean>
```

### 3. Volume 相关

```typescript
// 列出卷
listVolumes(projectId: string): Promise<Volume[]>

// 创建卷
createVolume(projectId: string, name: string): Promise<Volume>
```

### 4. Chapter 相关

```typescript
// 列出章节
listChapters(volumeId: string): Promise<Chapter[]>

// 加载章节内容
loadChapter(chapterId: string): Promise<ChapterData>

// 保存章节内容
saveChapter(chapterId: string, text: string): Promise<SaveReceipt>

// 创建章节
createChapter(volumeId: string, name: string): Promise<Chapter>

// 删除章节
deleteChapter(chapterId: string): Promise<boolean>
```

### 5. StarMap 相关

```typescript
// 列出星图
listStarMaps(): Promise<StarMapMeta[]>

// 获取星图图数据
getStarMapGraph(starmapId: string): Promise<StarMapGraph>
```

### 6. Settings 相关

```typescript
// 获取本地设置
getLocalSettings(): Promise<LocalSettings>

// 保存本地设置
saveLocalSettings(settings: LocalSettings): Promise<boolean>
```

---

## 四、数据传输对象 (DTO)

### Workspace

```typescript
interface Workspace {
  path: string
  name: string
  isValid: boolean
}
```

### Project

```typescript
interface Project {
  id: string
  name: string
  createdAt: string
  updatedAt: string
}
```

### Volume

```typescript
interface Volume {
  id: string
  projectId: string
  name: string
  order: number
}
```

### Chapter

```typescript
interface Chapter {
  id: string
  volumeId: string
  name: string
  wordCount: number
  updatedAt: string
}
```

### ChapterData

```typescript
interface ChapterData {
  id: string
  title: string
  content: string
  wordCount: number
  updatedAt: string
}
```

### SaveReceipt

```typescript
interface SaveReceipt {
  success: boolean
  wordCount: number
  savedAt: string
  errorCode?: string
  errorMessage?: string
}
```

### StarMapMeta

```typescript
interface StarMapMeta {
  id: string
  title: string
  description: string
  nodeCount: number
  createdAt: string
}
```

### StarMapGraph

```typescript
interface StarMapGraph {
  id: string
  title: string
  nodes: StarMapNode[]
  edges: StarMapEdge[]
}

interface StarMapNode {
  id: string
  label: string
  x: number
  y: number
  type: string
}

interface StarMapEdge {
  id: string
  sourceId: string
  targetId: string
  label?: string
}
```

### LocalSettings

```typescript
interface LocalSettings {
  fontSize: number
  lineHeight: number
  fontFamily: string
  theme: 'light' | 'dark' | 'system'
  autoSave: boolean
  autoIndent: boolean
}
```

---

## 五、错误处理

### ResultEnvelope

所有接口返回统一的 ResultEnvelope 结构：

```typescript
interface ResultEnvelope<T> {
  success: boolean
  data?: T
  errorCode?: string
  userMessage?: string
  rawError?: string
}
```

### 标准错误码

| 错误码 | 说明 |
|--------|------|
| `WORKSPACE_NOT_FOUND` | 工作区不存在 |
| `WORKSPACE_LOCKED` | 工作区被锁定 |
| `PROJECT_NOT_FOUND` | 项目不存在 |
| `CHAPTER_NOT_FOUND` | 章节不存在 |
| `EMPTY_OVERWRITE_BLOCKED` | 空内容覆盖被阻止 |
| `SYNC_MERGE_CONFLICT` | 同步合并冲突 |
| `PERMISSION_DENIED` | 权限不足 |
| `IO_ERROR` | IO 错误 |
| `UNKNOWN_ERROR` | 未知错误 |

---

## 六、Mock 实现阶段

当前阶段，WriterCoreBridge 返回硬编码的 mock 数据，用于：

1. 验证页面结构和路由
2. 验证状态流和数据绑定
3. 验证 UI 组件渲染
4. 确认接口形状是否满足需求

Mock 数据不涉及真实文件系统或 Rust Core。

---

## 七、未来接入方案

### 方案 A：C-ABI + NAPI

```
Rust Core → C ABI (.so) → NAPI → ArkTS
```

**优点**：
- 性能最优
- 与 Android UniFFI 路线一致

**缺点**：
- 需要鸿蒙 NAPI 工具链支持
- 需要处理 native 生命周期

### 方案 B：本地服务

```
Rust Core → 本地 HTTP/gRPC 服务 → ArkTS HTTP Client
```

**优点**：
- 进程隔离，稳定性好
- 不依赖 native 工具链

**缺点**：
- 性能开销较大
- 需要处理服务生命周期

### 方案 C：UniFFI 扩展

如果 UniFFI 支持鸿蒙目标：

```
Rust Core → UniFFI → ArkTS binding
```

**优点**：
- 与 Android 完全一致
- 维护成本最低

**缺点**：
- 依赖 UniFFI 鸿蒙支持
- 可能需要社区贡献

---

## 八、参考
- [API_CONTRACTS.md](archive/API_CONTRACTS.md) - 接口边界与交互契约（已归档）

- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md) - 跨平台能力契约（已归档）
- [harmony_route.md](harmony_route.md) - 鸿蒙技术路线
