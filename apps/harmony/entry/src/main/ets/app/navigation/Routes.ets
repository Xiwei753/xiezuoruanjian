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
}

export interface StarMapRouteParams {
  starmapId: string
  starmapTitle: string
}
