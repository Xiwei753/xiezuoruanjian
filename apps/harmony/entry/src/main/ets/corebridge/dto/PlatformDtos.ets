// PlatformDtos — Harmony 平台自己的类型。
//
// Issue #920 D：WriterCore 不再提供布局契约 / 工作台计划 / 页面策略，
// 相关 DTO 与解码调用已删除。这里只保留鸿蒙平台自身的窗口 / 折叠输入类型。
//
// 页面角色、动作与动作区域见 app/navigation/ScreenSemantics.ets（平台 UI 语义）。

// Harmony 平台自身的折叠姿态类型（平台输入，不在 Core 契约）。
export enum FoldPosture {
  Unknown = 'Unknown',
  FullyOpened = 'FullyOpened',
  HalfOpened = 'HalfOpened',
  Closed = 'Closed'
}

// Harmony 平台自身的大小分类（平台决策，不在 Core 契约）。
export enum WidthClass {
  Compact = 'Compact',
  Medium = 'Medium',
  Expanded = 'Expanded'
}
