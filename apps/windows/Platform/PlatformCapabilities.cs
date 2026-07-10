// Windows 平台适配层
//
// 将 CoreTextEditContext / TSF / WinRT Clipboard / 焦点 / 候选框等系统交互
// 收敛到此命名空间，SujianEditor 只调用统一接口，不直接知道 WinRT 细节。
//
// Windows 不复用 Linux Qt 平台逻辑，只复用 Core 的 EditorVisualTransaction 语义。
//
// 能力声明必须与 core/writer_core/src/platform_interaction/capabilities.rs 的 windows() 工厂方法对齐。
// 未真实接入的能力必须为 false，不允许吹牛。

namespace Sujian.Platform
{
    /// <summary>
    /// 平台能力集合 — 启动时由平台适配层一次性报告
    ///
    /// 与 Core PlatformCapabilities::windows() 对齐：
    /// - IME preedit: CoreTextEditContext composition/commit ✓
    /// - cursor anchor: CoreTextEditContext + candidate window anchoring ✓
    /// - replacement commit: CoreTextEditContext 未实现 replacement range commit ✗
    /// - text animation: SujianAnimationOverlay 未接入 Core visual transaction ✗
    /// - smooth cursor: cursor blink 仅有闪烁，无平滑移动动画 ✗
    /// - reflow animation: 未接入 Core reflow visual transaction ✗
    /// - clipboard: Windows.ApplicationModel.DataTransfer.Clipboard ✓
    /// - context menu: WinUI 3 context menu 未通过适配器接入 ✗
    /// - IEditorTransactionBoundary: LocalStandaloneTransactionBoundary (UsesCoreEngine == false)
    ///   SujianEditor 编辑路径已全部走 IEditorTransactionBoundary，
    ///   不再直接操作 _lines。
    /// </summary>
    public class PlatformCapabilities
    {
        public bool SupportsImePreedit { get; init; } = true;
        public bool SupportsCursorAnchor { get; init; } = true;
        public bool SupportsReplacementCommit { get; init; } = false;
        public bool SupportsTextAnimation { get; init; } = false;
        public bool SupportsSmoothCursor { get; init; } = false;
        public bool SupportsReflowAnimation { get; init; } = false;
        public bool SupportsClipboard { get; init; } = true;
        public bool SupportsContextMenu { get; init; } = false;

        public static PlatformCapabilities Windows() => new()
        {
            SupportsImePreedit = true,
            SupportsCursorAnchor = true,
            SupportsReplacementCommit = false,
            SupportsTextAnimation = false,
            SupportsSmoothCursor = false,
            SupportsReflowAnimation = false,
            SupportsClipboard = true,
            SupportsContextMenu = false,
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
