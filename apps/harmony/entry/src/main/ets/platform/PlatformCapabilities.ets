// Harmony 平台适配层
//
// 保留 DTO 和 bridge 签名，但 editorVisualTransaction 当前未实现。
// 设置页按 capabilities 禁用动画，不再放 TextArea overlay 假动画。
//
// Harmony 暂时能力不足，不假装实现。

/**
 * 平台能力集合 — 启动时由平台适配层一次性报告
 *
 * Harmony 动画未实现，设置页按 capabilities 禁用动画。
 */
export interface PlatformCapabilities {
  supportsImePreedit: boolean;
  supportsCursorAnchor: boolean;
  supportsReplacementCommit: boolean;
  supportsTextAnimation: boolean;
  supportsSmoothCursor: boolean;
  supportsReflowAnimation: boolean;
  supportsClipboard: boolean;
  supportsContextMenu: boolean;
}

/**
 * Harmony 平台能力 — 动画未实现，能力受限
 */
export function getHarmonyCapabilities(): PlatformCapabilities {
  return {
    supportsImePreedit: false,
    supportsCursorAnchor: false,
    supportsReplacementCommit: false,
    supportsTextAnimation: false,
    supportsSmoothCursor: false,
    supportsReflowAnimation: false,
    supportsClipboard: true,
    supportsContextMenu: true,
  };
}

/**
 * 判断是否有任何动画能力
 */
export function hasAnyAnimationSupport(caps: PlatformCapabilities): boolean {
  return caps.supportsTextAnimation || caps.supportsSmoothCursor || caps.supportsReflowAnimation;
}

/**
 * 归一化输入事件类型
 */
export enum NormalizedTextInputKind {
  PlainText = 'plainText',
  Shortcut = 'shortcut',
  PreeditChanged = 'preeditChanged',
  ImeCommit = 'imeCommit',
  ImeReplacementCommit = 'imeReplacementCommit',
  ImeCancel = 'imeCancel',
}

/**
 * 动画暂停原因
 */
export enum AnimationSuppressReason {
  Scrolling = 'scrolling',
  LoadingChapter = 'loadingChapter',
  SwitchingChapter = 'switchingChapter',
  ChangingFontSize = 'changingFontSize',
  ChangingTheme = 'changingTheme',
  ApplyingSettings = 'applyingSettings',
  AnimationDisabled = 'animationDisabled',
  WindowMinimized = 'windowMinimized',
  WindowHidden = 'windowHidden',
}

/**
 * 归一化光标矩形
 */
export interface NormalizedCursorRect {
  x: number;
  top: number;
  bottom: number;
  baselineY: number;
}

/**
 * 焦点状态
 */
export interface FocusState {
  hasFocus: boolean;
  softInputVisible: boolean;
}
