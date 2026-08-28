package com.xiwei.sujian.feature.editor.session

/**
 * Composable configuration for an editable text target's input and display behavior.
 *
 * Per #541: profile determines how the editing host configures itself for a
 * given target — it does NOT determine where the text is saved (that is the domain command's
 * responsibility). Multiple targets can share the same profile (e.g. project title and chapter
 * title both use [ShortTitle]) while committing to different domain objects.
 *
 * #641：profile 由 [WritingEditorSurface] 的 state-based [BasicTextField] 在每次
 * [bindSession] 时应用，切换 target 会完全重配置编辑器。
 */
data class TextEditorProfile(
    val singleLine: Boolean = false,
    val minLines: Int = 1,
    val maxLines: Int = if (singleLine) 1 else Int.MAX_VALUE,
    val horizontalScroll: Boolean = false,
    val verticalScroll: Boolean = !singleLine,
    val inputType: TextInputType = TextInputType.TEXT,
    val imeAction: ImeAction = if (singleLine) ImeAction.DONE else ImeAction.NONE,
    val newlinePolicy: NewlinePolicy = if (singleLine) NewlinePolicy.FORBID else NewlinePolicy.ALLOW,
    val autoIndentPolicy: AutoIndentPolicy = AutoIndentPolicy.NONE,
    val maxLength: Int = 0,
    val animationPolicy: AnimationPolicy = AnimationPolicy.INHERIT_GLOBAL,
    val cursorPolicy: CursorPolicy = CursorPolicy.VISIBLE_WHEN_FOCUSED,
    val selectionPolicy: SelectionPolicy = SelectionPolicy.ALLOW,
    val commitOnImeAction: Boolean = true,
    val commitOnFocusLoss: Boolean = true,
    val autocorrectPolicy: AutocorrectPolicy = AutocorrectPolicy.DEFAULT,
    val capitalizationPolicy: CapitalizationPolicy = CapitalizationPolicy.NONE,
    val copyPolicy: CopyPolicy = CopyPolicy.ALLOW,
    val pastePolicy: PastePolicy = PastePolicy.ALLOW,
    val secretPolicy: SecretPolicy = SecretPolicy.NONE,
    val fontSizePx: Float = 48f,
    val lineSpacingMultiplier: Float = 1.0f,
) {
    companion object {
        val DocumentBody =
            TextEditorProfile(
                singleLine = false,
                verticalScroll = true,
                inputType = TextInputType.MULTI_LINE,
                imeAction = ImeAction.NONE,
                newlinePolicy = NewlinePolicy.ALLOW,
                // #624 评论3：写作正文不继承上一行前导空白 — “自动首行缩进”是显示层
                // 排版（ParagraphStyleProjection 的 FirstLineIndentSpan），不是代码
                // 编辑器 Enter 后复制空格/Tab 的 code-style auto-indent。
                autoIndentPolicy = AutoIndentPolicy.NONE,
                animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
                commitOnFocusLoss = false,
            )
        val ShortTitle =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 200,
                animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
            )
        val ShortDescription =
            TextEditorProfile(
                singleLine = false,
                minLines = 1,
                maxLines = 4,
                inputType = TextInputType.MULTI_LINE,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.ALLOW,
                maxLength = 2000,
                animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
            )
        val InlineLabel =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 100,
                animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
            )
        val CanvasLabel =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 200,
                animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
            )
        val SearchQuery =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.SEARCH,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 500,
                animationPolicy = AnimationPolicy.SYSTEM_SUPPRESSED,
                commitOnFocusLoss = true,
            )
        val LongNote =
            TextEditorProfile(
                singleLine = false,
                minLines = 3,
                maxLines = 20,
                inputType = TextInputType.MULTI_LINE,
                imeAction = ImeAction.NONE,
                newlinePolicy = NewlinePolicy.ALLOW,
                animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
                commitOnFocusLoss = false,
            )
        val SecretToken =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.PASSWORD,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                animationPolicy = AnimationPolicy.SYSTEM_SUPPRESSED,
                selectionPolicy = SelectionPolicy.CURSOR_ONLY,
                copyPolicy = CopyPolicy.BLOCK,
                pastePolicy = PastePolicy.ALLOW,
                secretPolicy = SecretPolicy.MASK_AND_CLEAR_ON_COMMIT,
                commitOnFocusLoss = true,
            )
        val RepositoryUrl =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 500,
                animationPolicy = AnimationPolicy.SYSTEM_SUPPRESSED,
                autocorrectPolicy = AutocorrectPolicy.DISABLED,
                capitalizationPolicy = CapitalizationPolicy.NONE,
                commitOnFocusLoss = true,
            )
        val BranchName =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 200,
                animationPolicy = AnimationPolicy.SYSTEM_SUPPRESSED,
                autocorrectPolicy = AutocorrectPolicy.DISABLED,
                capitalizationPolicy = CapitalizationPolicy.NONE,
                commitOnFocusLoss = true,
            )
        val ReplaceQuery =
            TextEditorProfile(
                singleLine = true,
                inputType = TextInputType.TEXT,
                imeAction = ImeAction.DONE,
                newlinePolicy = NewlinePolicy.FORBID,
                maxLength = 500,
                animationPolicy = AnimationPolicy.SYSTEM_SUPPRESSED,
                autocorrectPolicy = AutocorrectPolicy.DISABLED,
                commitOnFocusLoss = true,
            )
    }
}

enum class TextInputType {
    TEXT,
    MULTI_LINE,
    NUMBER,
    EMAIL,
    PASSWORD,
}

enum class ImeAction {
    NONE,
    DONE,
    SEARCH,
    NEXT,
    GO,
}

enum class NewlinePolicy {
    ALLOW,
    FORBID,
}

enum class AutoIndentPolicy {
    NONE,
    INDENT_ON_ENTER,
}

enum class AnimationPolicy {
    INHERIT_GLOBAL,
    ENABLED,
    SYSTEM_SUPPRESSED,
}

enum class CursorPolicy {
    VISIBLE_WHEN_FOCUSED,
    HIDDEN,
}

enum class SelectionPolicy {
    ALLOW,
    CURSOR_ONLY,
}

/**
 * Commit timing policy for an editable text target.
 *
 * Per #541: determines when the coordinator submits the current text to the domain model.
 * - [COMMIT_ON_CONFIRM]: text is submitted only on explicit commit (IME action, focus loss,
 *   or programmatic commitActiveEdit). Used for draft sessions (project title, search query)
 *   where the domain model should not see intermediate states.
 * - [COMMIT_ON_EVERY_CHANGE]: text is submitted on every content change callback. Used for
 *   persistent sessions (chapter body) where the domain model must stay in sync with edits.
 */
enum class CommitPolicy {
    COMMIT_ON_CONFIRM,
    COMMIT_ON_EVERY_CHANGE,
}

enum class AutocorrectPolicy {
    DEFAULT,
    DISABLED,
}

enum class CapitalizationPolicy {
    NONE,
    CHARACTERS,
    WORDS,
    SENTENCES,
}

enum class CopyPolicy {
    ALLOW,
    BLOCK,
}

enum class PastePolicy {
    ALLOW,
    BLOCK,
}

enum class SecretPolicy {
    NONE,
    MASK_AND_CLEAR_ON_COMMIT,
}
