// Routes — 应用路由枚举与参数类型。
// 页面通过统一 Navigation（AppNavigation）进出，不再用 router 跳转。

export enum Routes {
  Home = 'Home',
  Workspace = 'Workspace',
  Writing = 'Writing',
  StarMap = 'StarMap',
  Settings = 'Settings',
  Stats = 'Stats'
}

export interface WorkspaceRouteParams {
  projectId: string
  projectName: string
}

export interface WritingRouteParams {
  chapterId: string
  chapterTitle: string
  projectId: string
  volumeId: string
  // 接续编辑器状态（Issue #629 评论 5306513458 问题4）。
  // selection/cursor 是 UTF-16 code unit offset（UI 坐标，跨设备平台无关）；
  // 恢复时由 WritingScreen 转成 Core UTF-8 byte offset。
  selection?: number[]   // [anchorUtf16, headUtf16]
  cursor?: number         // UTF-16 code unit offset
  viewport?: object       // UI 滚动位置（不写 Core）
  revision?: number       // Core 正文 revision
}

export interface StarMapRouteParams {
  starmapId: string
  starmapTitle: string
}
