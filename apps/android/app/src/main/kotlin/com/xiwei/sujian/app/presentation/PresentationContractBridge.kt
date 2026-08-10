package com.xiwei.sujian.app.presentation

import android.content.Context
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.LayoutContractDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto
import uniffi.writer_core.WindowCapabilitiesDto

/**
 * Presentation Contract Bridge — Android 侧消费 Core presentation contract 的唯一入口（#610）。
 *
 * 分层（#610）：
 * - Core `presentation/layout_contract.rs` + `presentation/screen_contract.rs` 是
 *   平台无关的产品界面契约唯一事实来源；
 * - 本桥只做两件事：把 Android 需要的能力/角色 DTO 传给 Core，把 Core 返回的
 *   contract DTO 转成 Android 可消费的形式；
 * - 图标、颜色、TopAppBar、NavigationBar、NavigationRail 等具体控件全部留在
 *   Android presentation/render 层（SujianNavigationSuite + AndroidChromePolicy）。
 *
 * 旧的 ScreenPolicyBridge / LayoutPolicyBridge / ScreenPolicyModels /
 * LayoutPolicyModels 已删除，这里只有一个入口。
 *
 * #610 评论二：ActionSlotDto 携带平台无关的业务目标身份（target），
 * Delete/Rename 等动作可区分“删卷/删章节/重命名卷/重命名章节”，
 * Android 消费端直接读 DTO 字段即可绑定业务操作，不靠区域/顺序猜身份。
 */
internal object PresentationContractBridge {
    private const val TAG = "PresentationContractBridge"

    /** Core 布局契约：窗口能力 → ShellMode/WorkspacePaneMode。 */
    fun resolveLayoutContract(
        context: Context,
        capabilities: WindowCapabilitiesDto,
    ): LayoutContractDto? = resolve(context) { bridge -> bridge.resolveLayout(capabilities) }

    /** Core 页面契约：ScreenRole → ActionSlot 列表（区域/顺序是产品语义）。 */
    fun resolveScreenPolicy(
        context: Context,
        screenRole: ScreenRoleDto,
    ): ScreenPolicyDto? = resolve(context) { bridge -> bridge.resolveScreenPolicy(screenRole) }

    private fun <T> resolve(
        context: Context,
        call: (AppServiceBridge) -> BridgeResult<T>,
    ): T? =
        try {
            when (val result = call(AppServiceProvider.getAppServiceBridge(context))) {
                is BridgeResult.Success -> result.data
                else -> null
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "resolve failed: ${e.message}", e)
            null
        }

    // ── Android 消费端便捷转换（同一入口，避免各 UI 层重复映射） ──

    /** HeaderTrailing 区域的槽位，按 order 升序。 */
    fun headerTrailingSlots(policy: ScreenPolicyDto?): List<ActionSlotDto> =
        policy?.actionSlots
            ?.filter { it.region == ActionRegionDto.HEADER_TRAILING }
            ?.sortedBy { it.order.toInt() }
            .orEmpty()

    /** HeaderLeading 区域是否包含指定动作角色。 */
    fun hasRoleAtLeading(
        policy: ScreenPolicyDto?,
        role: ActionRoleDto,
    ): Boolean =
        policy?.actionSlots
            ?.any { it.region == ActionRegionDto.HEADER_LEADING && it.role == role }
            ?: false
}
