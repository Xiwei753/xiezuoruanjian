// Windows 平台适配层
//
// 将 CoreTextEditContext / TSF / WinRT Clipboard / 焦点 / 候选框等系统交互
// 收敛到此命名空间，SujianEditor 只调用统一接口，不直接知道 WinRT 细节。
//
// Windows 不复用 Linux Qt 平台逻辑，只复用 Core 的 EditorVisualTransaction 语义。

namespace Sujian.Platform
{
    /// <summary>
    /// 平台能力集合 — 启动时由平台适配层一次性报告
    /// </summary>
    public class PlatformCapabilities
    {
        public bool SupportsImePreedit { get; init; } = true;
        public bool SupportsCursorAnchor { get; init; } = true;
        public bool SupportsReplacementCommit { get; init; } = false;
        public bool SupportsTextAnimation { get; init; } = true;
        public bool SupportsSmoothCursor { get; init; } = true;
        public bool SupportsReflowAnimation { get; init; } = true;
        public bool SupportsClipboard { get; init; } = true;
        public bool SupportsContextMenu { get; init; } = true;

        public static PlatformCapabilities Windows() => new()
        {
            SupportsImePreedit = true,
            SupportsCursorAnchor = true,
            SupportsReplacementCommit = false,
            SupportsTextAnimation = true,
            SupportsSmoothCursor = true,
            SupportsReflowAnimation = true,
            SupportsClipboard = true,
            SupportsContextMenu = true,
        };

        public bool HasAnyAnimationSupport() =>
            SupportsTextAnimation || SupportsSmoothCursor || SupportsReflowAnimation;

        public bool HasAnyImeSupport() =>
            SupportsImePreedit || SupportsCursorAnchor || SupportsReplacementCommit;
    }

    /// <summary>
    /// 归一化输入事件 — 平台适配层输出，编辑器消费
    /// </summary>
    public enum NormalizedTextInputKind
    {
        PlainText,
        Shortcut,
        PreeditChanged,
        ImeCommit,
        ImeReplacementCommit,
        ImeCancel,
    }

    /// <summary>
    /// 动画暂停原因
    /// </summary>
    public enum AnimationSuppressReason
    {
        Scrolling,
        LoadingChapter,
        SwitchingChapter,
        ChangingFontSize,
        ChangingTheme,
        ApplyingSettings,
        AnimationDisabled,
        WindowMinimized,
        WindowHidden,
    }

    /// <summary>
    /// 归一化光标矩形
    /// </summary>
    public record NormalizedCursorRect(double X, double Top, double Bottom, double BaselineY);

    /// <summary>
    /// 焦点状态
    /// </summary>
    public record FocusState(bool HasFocus, bool SoftInputVisible);
}
