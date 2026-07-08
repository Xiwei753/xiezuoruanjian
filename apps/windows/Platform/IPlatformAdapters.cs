// Windows TextInputAdapter 接口
//
// CoreTextEditContext / CharacterReceived / TextUpdating 收敛到此。
// SujianEditor 只调用统一接口，不直接知道 WinRT 输入细节。

using System;

namespace Sujian.Platform
{
    /// <summary>
    /// Windows TextInputAdapter 接口
    /// </summary>
    public interface ITextInputAdapter
    {
        /// <summary>
        /// 当前是否正在 IME composing
        /// </summary>
        bool IsImeComposing { get; }

        /// <summary>
        /// 是否可以接受纯文本按键
        /// </summary>
        bool CanAcceptPlainTextKey => !IsImeComposing;

        /// <summary>
        /// UTF-16 offset → UTF-8 byte offset 转换
        /// </summary>
        int Utf16ToUtf8Offset(string text, int utf16Offset);

        /// <summary>
        /// UTF-8 byte offset → UTF-16 offset 转换
        /// </summary>
        int Utf8ToUtf16Offset(string text, int utf8Offset);
    }

    /// <summary>
    /// Windows CursorAnchorAdapter 接口
    /// </summary>
    public interface ICursorAnchorAdapter
    {
        /// <summary>
        /// 通知系统输入法光标/选区已更新
        /// </summary>
        void NotifyCursorAnchorUpdate(
            int cursorIndex,
            int anchorIndex,
            int selectionStart,
            int selectionEnd,
            string textBeforeCursor,
            string textAfterCursor);

        /// <summary>
        /// 请求系统更新候选框位置
        /// </summary>
        void RequestCandidateWindowUpdate(NormalizedCursorRect cursorRect);
    }

    /// <summary>
    /// Windows AnimationDriver 接口
    /// </summary>
    public interface IAnimationDriver
    {
        /// <summary>
        /// 报告当前是否应该抑制动画
        /// </summary>
        bool ShouldSuppressAnimation();

        /// <summary>
        /// 通知动画暂停
        /// </summary>
        void NotifyAnimationSuppressed(AnimationSuppressReason reason);

        /// <summary>
        /// 通知动画恢复
        /// </summary>
        void NotifyAnimationResumed();

        /// <summary>
        /// 取消所有进行中的动画
        /// </summary>
        void CancelAllAnimations();

        /// <summary>
        /// 请求立即完成所有动画
        /// </summary>
        void FinishAllAnimations();
    }

    /// <summary>
    /// Windows ClipboardAndFocusAdapter 接口
    /// </summary>
    public interface IClipboardAndFocusAdapter
    {
        /// <summary>
        /// 执行剪贴板操作
        /// </summary>
        string? ExecuteClipboard(ClipboardOperation operation, string? text = null);

        /// <summary>
        /// 执行焦点请求
        /// </summary>
        void ExecuteFocus(FocusRequest request);

        /// <summary>
        /// 获取当前焦点状态
        /// </summary>
        FocusState GetFocusState();
    }

    /// <summary>
    /// 剪贴板操作类型
    /// </summary>
    public enum ClipboardOperation
    {
        Copy,
        Paste,
        Cut,
        HasText,
    }

    /// <summary>
    /// 焦点请求类型
    /// </summary>
    public enum FocusRequest
    {
        RequestFocus,
        ReleaseFocus,
        RequestSoftInput,
        HideSoftInput,
    }
}
