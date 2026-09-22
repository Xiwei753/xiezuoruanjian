// IEditorTransactionBoundary — Windows 正文编辑语义收口
//
// Windows SujianEditor 不能继续自己维护正文编辑语义。
// 正文变更要接入 Core 编辑事务（EditorEditResult：cause / operationKind / offsetMap）。
// Windows 只做 CoreTextEditContext、DirectWrite/Direct2D 布局绘制、hit test、候选框锚点和动画显示。
//
// 此接口定义 Core editor transaction 的调用边界。
// SujianEditor 通过此接口委托正文变更，不再自己直接操作 _lines。
//
// Issue #735：Core 不再提供 EditorVisualTransaction / AnimationMode 等视觉类型。
// Windows 从 EditorEditResult 的 cause / operationKind 推导动画策略，
// 用自己的 DirectWrite text layout 生成 ghost glyph 几何。
//
// ⚠️ 当前唯一实现 LocalStandaloneTransactionBoundary 是本地独立实现，
//    未接入 Core 编辑事务（UsesCoreEngine == false）。
//    它做基本字符串操作，功能正确但不含 Core 语义（undo stack、offset map 等）。
//    新增编辑路径必须通过 IEditorTransactionBoundary，不允许绕过。
//    接入 Core 编辑事务后应替换为 CoreEditorTransactionBoundary 实现。

using System;

namespace Sujian.Windows.Editor
{
    /// <summary>
    /// 编辑操作类型
    /// </summary>
    public enum EditorTransactionCause
    {
        Typing,
        DeleteBackward,
        DeleteForward,
        ImeCommit,
        ImePreedit,
        Paste,
        Cut,
        Undo,
        Redo,
        Replace,
        Other,
    }

    /// <summary>
    /// 编辑事务结果
    ///
    /// Issue #735：Core 不再返回 visual transaction JSON。
    /// Windows 从 cause 推导动画策略，用自己的 text layout 生成几何。
    /// </summary>
    public sealed class EditorTransactionResult
    {
        public string NewText { get; init; } = string.Empty;
        public int NewCursorOffset { get; init; }
        public int NewAnchorOffset { get; init; }
        public bool ShouldAnimate { get; init; }
    }

    /// <summary>
    /// Core editor transaction 调用边界
    ///
    /// SujianEditor 必须通过此接口执行正文变更。
    /// 禁止新增直接操作 _lines 的编辑路径。
    /// </summary>
    public interface IEditorTransactionBoundary
    {
        /// <summary>
        /// 执行插入文本事务
        /// </summary>
        EditorTransactionResult InsertText(string oldText, int cursorOffset, int anchorOffset, string textToInsert, EditorTransactionCause cause);

        /// <summary>
        /// 执行删除后退事务
        /// </summary>
        EditorTransactionResult DeleteBackward(string oldText, int cursorOffset, int anchorOffset, EditorTransactionCause cause);

        /// <summary>
        /// 执行删除前进事务
        /// </summary>
        EditorTransactionResult DeleteForward(string oldText, int cursorOffset, int anchorOffset, EditorTransactionCause cause);

        /// <summary>
        /// 执行替换事务（用于 IME 等）
        /// </summary>
        EditorTransactionResult ReplaceRange(string oldText, int cursorOffset, int anchorOffset, int replaceStart, int replaceLength, string replacement, EditorTransactionCause cause);

        /// <summary>
        /// 是否使用 Core 事务引擎
        /// </summary>
        bool UsesCoreEngine { get; }
    }

    /// <summary>
    /// 本地独立实现 — 未接入 Core 编辑事务
    ///
    /// ⚠️ 此实现做基本字符串操作，功能正确但不包含 Core 语义：
    /// - 无 Core undo/redo stack
    /// - 无 Core offset map 生成
    /// - 无 Core animation cause 决策
    ///
    /// UsesCoreEngine == false 明确标识此实现不走 Core 引擎。
    /// 接入 Core 后应替换为 CoreEditorTransactionBoundary。
    /// 新增编辑路径必须通过 IEditorTransactionBoundary，不允许绕过。
    /// </summary>
    public sealed class LocalStandaloneTransactionBoundary : IEditorTransactionBoundary
    {
        public bool UsesCoreEngine => false;

        public EditorTransactionResult InsertText(string oldText, int cursorOffset, int anchorOffset, string textToInsert, EditorTransactionCause cause)
        {
            var newText = oldText[..cursorOffset] + textToInsert + oldText[cursorOffset..];
            var newCursor = cursorOffset + textToInsert.Length;
            return new EditorTransactionResult
            {
                NewText = newText,
                NewCursorOffset = newCursor,
                NewAnchorOffset = newCursor,
                ShouldAnimate = cause == EditorTransactionCause.Typing || cause == EditorTransactionCause.ImeCommit,
            };
        }

        public EditorTransactionResult DeleteBackward(string oldText, int cursorOffset, int anchorOffset, EditorTransactionCause cause)
        {
            if (cursorOffset <= 0)
            {
                return new EditorTransactionResult
                {
                    NewText = oldText,
                    NewCursorOffset = cursorOffset,
                    NewAnchorOffset = anchorOffset,
                    ShouldAnimate = false,
                };
            }
            var newText = oldText[..(cursorOffset - 1)] + oldText[cursorOffset..];
            return new EditorTransactionResult
            {
                NewText = newText,
                NewCursorOffset = cursorOffset - 1,
                NewAnchorOffset = cursorOffset - 1,
                ShouldAnimate = true,
            };
        }

        public EditorTransactionResult DeleteForward(string oldText, int cursorOffset, int anchorOffset, EditorTransactionCause cause)
        {
            if (cursorOffset >= oldText.Length)
            {
                return new EditorTransactionResult
                {
                    NewText = oldText,
                    NewCursorOffset = cursorOffset,
                    NewAnchorOffset = anchorOffset,
                    ShouldAnimate = false,
                };
            }
            var newText = oldText[..cursorOffset] + oldText[(cursorOffset + 1)..];
            return new EditorTransactionResult
            {
                NewText = newText,
                NewCursorOffset = cursorOffset,
                NewAnchorOffset = cursorOffset,
                ShouldAnimate = true,
            };
        }

        public EditorTransactionResult ReplaceRange(string oldText, int cursorOffset, int anchorOffset, int replaceStart, int replaceLength, string replacement, EditorTransactionCause cause)
        {
            var actualStart = Math.Min(replaceStart, oldText.Length);
            var actualEnd = Math.Min(replaceStart + replaceLength, oldText.Length);
            var newText = oldText[..actualStart] + replacement + oldText[actualEnd..];
            var newCursor = actualStart + replacement.Length;
            return new EditorTransactionResult
            {
                NewText = newText,
                NewCursorOffset = newCursor,
                NewAnchorOffset = newCursor,
                ShouldAnimate = cause == EditorTransactionCause.ImeCommit,
            };
        }
    }
}
