// Harmony TextInputAdapter 接口
//
// Harmony 使用系统 TextArea，IME 交互由系统处理。
// 此接口保留签名，但当前未实现自研 IME 适配。

/**
 * TextInputAdapter 接口 — Harmony 暂不实现
 */
export interface TextInputAdapter {
  isImeComposing(): boolean;
  canAcceptPlainTextKey(): boolean;
  utf16ToUtf8Offset(text: string, utf16Offset: number): number;
  utf8ToUtf16Offset(text: string, utf8Offset: number): number;
}

/**
 * CursorAnchorAdapter 接口 — Harmony 暂不实现
 */
export interface CursorAnchorAdapter {
  notifyCursorAnchorUpdate(
    cursorIndex: number,
    anchorIndex: number,
    selectionStart: number,
    selectionEnd: number,
    textBeforeCursor: string,
    textAfterCursor: string
  ): void;
  requestCandidateWindowUpdate(cursorRect: NormalizedCursorRect): void;
}

/**
 * AnimationDriver 接口 — Harmony 暂不实现
 *
 * editorVisualTransaction 在 IWriterCoreBridge 中已定义但未使用。
 * 设置页按 capabilities 禁用动画，不再放 TextArea overlay 假动画。
 */
export interface AnimationDriver {
  shouldSuppressAnimation(): boolean;
  notifyAnimationSuppressed(reason: AnimationSuppressReason): void;
  notifyAnimationResumed(): void;
  cancelAllAnimations(): void;
  finishAllAnimations(): void;
}

/**
 * ClipboardAndFocusAdapter 接口
 */
export interface ClipboardAndFocusAdapter {
  executeClipboard(operation: ClipboardOperation, text?: string): string | undefined;
  executeFocus(request: FocusRequest): void;
  getFocusState(): FocusState;
}

/**
 * 剪贴板操作类型
 */
export enum ClipboardOperation {
  Copy = 'copy',
  Paste = 'paste',
  Cut = 'cut',
  HasText = 'hasText',
}

/**
 * 焦点请求类型
 */
export enum FocusRequest {
  RequestFocus = 'requestFocus',
  ReleaseFocus = 'releaseFocus',
  RequestSoftInput = 'requestSoftInput',
  HideSoftInput = 'hideSoftInput',
}
