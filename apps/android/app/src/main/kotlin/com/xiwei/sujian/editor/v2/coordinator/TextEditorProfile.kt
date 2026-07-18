package com.xiwei.sujian.editor.v2.coordinator

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
    val commitOnFocusLoss: Boolean = true
) {
    companion object {
        val DocumentBody = TextEditorProfile(
            singleLine = false,
            verticalScroll = true,
            inputType = TextInputType.MULTI_LINE,
            imeAction = ImeAction.NONE,
            newlinePolicy = NewlinePolicy.ALLOW,
            autoIndentPolicy = AutoIndentPolicy.INDENT_ON_ENTER,
            animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
            commitOnFocusLoss = false
        )
        val ShortTitle = TextEditorProfile(
            singleLine = true,
            inputType = TextInputType.TEXT,
            imeAction = ImeAction.DONE,
            newlinePolicy = NewlinePolicy.FORBID,
            maxLength = 200,
            animationPolicy = AnimationPolicy.INHERIT_GLOBAL
        )
        val ShortDescription = TextEditorProfile(
            singleLine = false,
            minLines = 1,
            maxLines = 4,
            inputType = TextInputType.MULTI_LINE,
            imeAction = ImeAction.DONE,
            newlinePolicy = NewlinePolicy.ALLOW,
            maxLength = 2000,
            animationPolicy = AnimationPolicy.INHERIT_GLOBAL
        )
        val InlineLabel = TextEditorProfile(
            singleLine = true,
            inputType = TextInputType.TEXT,
            imeAction = ImeAction.DONE,
            newlinePolicy = NewlinePolicy.FORBID,
            maxLength = 100,
            animationPolicy = AnimationPolicy.INHERIT_GLOBAL
        )
        val CanvasLabel = TextEditorProfile(
            singleLine = true,
            inputType = TextInputType.TEXT,
            imeAction = ImeAction.DONE,
            newlinePolicy = NewlinePolicy.FORBID,
            maxLength = 200,
            animationPolicy = AnimationPolicy.INHERIT_GLOBAL
        )
        val SearchQuery = TextEditorProfile(
            singleLine = true,
            inputType = TextInputType.TEXT,
            imeAction = ImeAction.SEARCH,
            newlinePolicy = NewlinePolicy.FORBID,
            maxLength = 500,
            animationPolicy = AnimationPolicy.SYSTEM_SUPPRESSED,
            commitOnFocusLoss = true
        )
        val LongNote = TextEditorProfile(
            singleLine = false,
            minLines = 3,
            maxLines = 20,
            inputType = TextInputType.MULTI_LINE,
            imeAction = ImeAction.NONE,
            newlinePolicy = NewlinePolicy.ALLOW,
            animationPolicy = AnimationPolicy.INHERIT_GLOBAL,
            commitOnFocusLoss = false
        )
    }
}

enum class TextInputType {
    TEXT,
    MULTI_LINE,
    NUMBER,
    EMAIL
}

enum class ImeAction {
    NONE,
    DONE,
    SEARCH,
    NEXT,
    GO
}

enum class NewlinePolicy {
    ALLOW,
    FORBID
}

enum class AutoIndentPolicy {
    NONE,
    INDENT_ON_ENTER
}

enum class AnimationPolicy {
    INHERIT_GLOBAL,
    ENABLED,
    SYSTEM_SUPPRESSED
}

enum class CursorPolicy {
    VISIBLE_WHEN_FOCUSED,
    HIDDEN
}

enum class SelectionPolicy {
    ALLOW,
    CURSOR_ONLY
}

enum class CommitPolicy {
    COMMIT_ON_CONFIRM,
    COMMIT_ON_EVERY_CHANGE
}
