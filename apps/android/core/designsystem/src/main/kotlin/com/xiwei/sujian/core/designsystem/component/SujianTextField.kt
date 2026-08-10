package com.xiwei.sujian.core.designsystem.component

import androidx.compose.material3.OutlinedTextField
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation

/**
 * 短文本表单输入框 — 封装 Material3 [OutlinedTextField]。
 *
 * 这是真正的 Compose TextField，自己建立焦点、InputConnection、软键盘输入链，
 * 不依赖任何编辑器窗口宿主。用于作品标题、同步 URL/branch/token 等短文本表单场景。
 *
 * @param value 当前文本值。
 * @param onValueChange 文本变化回调，每次按键都会触发。
 * @param label 可选标签内容。
 * @param modifier 修饰符。
 * @param enabled 是否启用。
 * @param singleLine 是否单行。
 */
@Composable
fun SujianTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: @Composable (() -> Unit)? = null,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    singleLine: Boolean = true,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier,
        enabled = enabled,
        singleLine = singleLine,
        label = label,
        visualTransformation = VisualTransformation.None,
    )
}

/**
 * 敏感短文本表单输入框 — 使用密码 visual transformation 遮蔽输入内容。
 *
 * 用于 token 等敏感输入场景。其余行为与 [SujianTextField] 一致。
 *
 * @param value 当前文本值。
 * @param onValueChange 文本变化回调，每次按键都会触发。
 * @param label 可选标签内容。
 * @param modifier 修饰符。
 * @param enabled 是否启用。
 * @param singleLine 是否单行。
 */
@Composable
fun SujianSecretTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: @Composable (() -> Unit)? = null,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    singleLine: Boolean = true,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier,
        enabled = enabled,
        singleLine = singleLine,
        label = label,
        visualTransformation = PasswordVisualTransformation(),
    )
}
