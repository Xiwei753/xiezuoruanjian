@file:Suppress("ktlint:standard:filename")

package com.xiwei.sujian.app.presentation.layout

import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import uniffi.writer_core.WindowOcclusionDto
import uniffi.writer_core.WindowViewportDto

/**
 * #640 评论 5443789509：稳定安全工作区视口 frame — Workbench planner 输入坐标系闭合修复。
 *
 * wide Workbench 坐标系之前未闭合：Rust 看到的 (0,0) 是物理窗口左上角，于是
 * `toolbar_height_dp=64` 实际只有 (64 - statusBarHeight) dp 的内容高度，状态栏
 * 露出下面透明 Scaffold。本 frame 把稳定系统 UI（systemBars + displayCutout）
 * 的 safe viewport 放到 planner 输入前：
 * - [originXDp]/[originYDp] 是稳定安全工作区左上角相对物理窗口的偏移；
 * - [viewport] 是裁掉稳定 inset 后的 safe viewport，occlusions 也平移到 safe 坐标系；
 * - planner 用 safe viewport 算 plan 后，Android 再用 [originXDp]/[originYDp]
 *   整体 `offsetBy` 平移回物理窗口坐标。
 *
 * 只用 systemBars + displayCutout，**不用 safeDrawing**：safeDrawing 含 IME，
 * 键盘开关会让 Rust 重算 workbench plan，但 IME 应留在 Editor 层动态处理
 * （EditorPane 用 `WindowInsetsRulers.Ime.current`），键盘出现只改变 Editor
 * 实际可用高度，不重算 Rust workbench plan。
 */
internal data class AndroidWorkbenchViewportFrame(
    val originXDp: Float,
    val originYDp: Float,
    val viewport: WindowViewportDto,
)

/**
 * #640 评论 5443789509：把 [rawViewport] 裁掉稳定 inset，得到 [AndroidWorkbenchViewportFrame]。
 *
 * 纯函数（无 Compose 副作用），便于单测：
 * - safe viewport width = `(raw.widthDp - leftDp - rightDp).coerceAtLeast(0f)`；
 * - safe viewport height = `(raw.heightDp - topDp - bottomDp).coerceAtLeast(0f)`；
 * - occlusions 逐个平移到 safe 坐标系（`leftDp -= leftDp`, `topDp -= topDp`,
 *   `rightDp -= leftDp`, `bottomDp -= topDp`，separating 不变）；
 * - 返回 `frame(originXDp = leftDp, originYDp = topDp, viewport = safeViewport)`。
 *
 * @param rawViewport 物理窗口原始视口（dp + occlusions）
 * @param leftDp 稳定系统 UI 左 inset（状态栏 / 导航栏 / 刘海左侧）
 * @param topDp 稳定系统 UI 顶 inset（状态栏 / 顶部刘海）
 * @param rightDp 稳定系统 UI 右 inset
 * @param bottomDp 稳定系统 UI 底 inset（导航栏 / 底部刘海，**不含 IME**）
 */
internal fun resolveSafeWorkbenchViewport(
    rawViewport: WindowViewportDto,
    leftDp: Float,
    topDp: Float,
    rightDp: Float,
    bottomDp: Float,
): AndroidWorkbenchViewportFrame {
    val safeWidth = (rawViewport.widthDp - leftDp - rightDp).coerceAtLeast(0f)
    val safeHeight = (rawViewport.heightDp - topDp - bottomDp).coerceAtLeast(0f)
    val safeOcclusions =
        rawViewport.occlusions.map { o ->
            WindowOcclusionDto(
                leftDp = o.leftDp - leftDp,
                topDp = o.topDp - topDp,
                rightDp = o.rightDp - leftDp,
                bottomDp = o.bottomDp - topDp,
                separating = o.separating,
            )
        }
    val safeViewport =
        WindowViewportDto(
            widthDp = safeWidth,
            heightDp = safeHeight,
            occlusions = safeOcclusions,
        )
    return AndroidWorkbenchViewportFrame(
        originXDp = leftDp,
        originYDp = topDp,
        viewport = safeViewport,
    )
}

/**
 * #640 评论 5443789509：用当前 Compose 宿主的稳定系统 inset 构造 [AndroidWorkbenchViewportFrame]。
 *
 * `stableInsets = WindowInsets.systemBars.union(WindowInsets.displayCutout)`。
 * 只用 systemBars + displayCutout，**不用 safeDrawing**：safeDrawing 含 IME，
 * 键盘开关不应让 Rust 重算 workbench plan；IME 留给 Editor 层（EditorPane 用
 * `WindowInsetsRulers.Ime.current`）动态处理。
 *
 * inset px → dp 用 [LocalDensity] 转，`getLeft`/`getRight` 传 [LocalLayoutDirection]，
 * `getTop`/`getBottom` 不传 layoutDirection。`remember(rawViewport, leftDp, topDp,
 * rightDp, bottomDp)` 缓存 frame，避免每帧重建对象导致下游 planner remember key 抖动。
 */
@Composable
internal fun rememberAndroidWorkbenchViewportFrame(rawViewport: WindowViewportDto): AndroidWorkbenchViewportFrame {
    val density = LocalDensity.current
    val layoutDirection = LocalLayoutDirection.current
    val stableInsets = WindowInsets.systemBars.union(WindowInsets.displayCutout)
    val leftDp = with(density) { stableInsets.getLeft(density, layoutDirection).toDp().value }
    val topDp = with(density) { stableInsets.getTop(density).toDp().value }
    val rightDp = with(density) { stableInsets.getRight(density, layoutDirection).toDp().value }
    val bottomDp = with(density) { stableInsets.getBottom(density).toDp().value }
    return remember(rawViewport, leftDp, topDp, rightDp, bottomDp) {
        resolveSafeWorkbenchViewport(rawViewport, leftDp, topDp, rightDp, bottomDp)
    }
}

/**
 * #640 评论 5444584755：[AndroidWorkbenchViewportFrame] 对应的物理安全矩形（dp）。
 *
 * safe frame 把稳定系统 UI（systemBars + displayCutout）裁掉后得到 safe viewport，
 * [originXDp]/[originYDp] 是安全区左上角相对物理窗口的偏移。本属性把 safe viewport
 * 还原成物理窗口坐标系下的安全矩形（left=originXDp, top=originYDp,
 * right=originXDp+width, bottom=originYDp+height），供 wide SinglePane / EditorOnly
 * 在 `workbenchPlan == null`（resolver 失败）时作为统一 fallback：
 * - plan 非空且 Editor bounds 非空 → 用 Rust plan 的 Editor bounds；
 * - plan null 或 bounds 空 → 用本矩形，不再回落整个 constraints（物理窗口 (0,0)）。
 *
 * plan 有无都共用同一份物理安全矩形，避免 resolver 失败时 TopAppBar 从物理窗口 (0,0)
 * 开始、正文只躲 IME 不躲 status bar / display cutout / navigation bar 的第二套坐标系。
 *
 * 纯属性（无 Compose 副作用），同包 internal，[AndroidLayoutRect] 在同包不需 import。
 */
internal val AndroidWorkbenchViewportFrame.physicalSafeBounds: AndroidLayoutRect
    get() =
        AndroidLayoutRect(
            leftDp = originXDp,
            topDp = originYDp,
            rightDp = originXDp + viewport.widthDp,
            bottomDp = originYDp + viewport.heightDp,
        )
