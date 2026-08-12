package com.xiwei.sujian.app.presentation

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.LayoutContractDto
import uniffi.writer_core.ScreenPolicyDto
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
 * #618 一：静态页面契约（ScreenRole → ScreenPolicy）的临时解析已由
 * [PresentationPolicyCatalog] 在应用容器创建时一次性完成，Compose 热路径只查
 * catalog，不再走本桥。本桥保留动态的布局契约解析（依赖窗口能力）。
 *
 * #610 评论二：ActionSlotDto 携带平台无关的业务目标身份（target），
 * Delete/Rename 等动作可区分“删卷/删章节/重命名卷/重命名章节”，
 * Android 消费端直接读 DTO 字段即可绑定业务操作，不靠区域/顺序猜身份。
 */
internal object PresentationContractBridge {
    private const val TAG = "PresentationContractBridge"

    /** Core 布局契约：窗口能力 → ShellMode/WorkspacePaneMode。 */
    fun resolveLayoutContract(
        bridge: AppServiceBridge,
        capabilities: WindowCapabilitiesDto,
    ): LayoutContractDto? =
        try {
            when (val result = bridge.resolveLayout(capabilities)) {
                is BridgeResult.Success -> result.data
                else -> null
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "resolveLayoutContract failed: ${e.message}", e)
            null
        }

    /**
     * #618 评论四：创建绑定 bridge 的布局契约解析器（函数类型）。
     *
     * 供 DI/navigation 层注入到 rememberAndroidLayoutSpec；presentation 层
     * （AndroidAdaptiveLayoutPolicy.kt）只消费函数类型，不直接依赖 AppServiceBridge，
     * 遵守架构门禁 presentation-contract-layer（只有本文件可以引用 Bridge）。
     */
    fun layoutContractResolver(bridge: AppServiceBridge): (WindowCapabilitiesDto) -> LayoutContractDto? {
        return { capabilities -> resolveLayoutContract(bridge, capabilities) }
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
