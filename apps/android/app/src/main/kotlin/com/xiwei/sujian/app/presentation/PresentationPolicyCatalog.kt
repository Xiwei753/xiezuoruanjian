package com.xiwei.sujian.app.presentation

import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto

/**
 * 页面契约目录 — 静态 `ScreenRoleDto -> ScreenPolicyDto` 的一次性解析（#618 一）。
 *
 * 页面角色到页面契约的映射是纯静态产品语义（Core screen_contract），不随窗口、
 * 导航位置或组合帧变化。应用容器创建时把六个 Android 实际消费的角色一次解析进
 * 内存 Map；之后 Compose 只查 Map，不再在页面组合热路径上临时跨 UniFFI 取契约。
 *
 * 这样章节树的卷章动作固定按 `PROJECT_WORKSPACE` 契约渲染，不会出现
 * "页面已经进了章节树、动作却还拿着 PROJECT_LIST 规格"的帧错位（#618 一）。
 *
 * [resolver] 由 DI 层注入（DefaultAppServiceContainer 用 AppServiceBridge 实现）；
 * 本类不直接依赖 Bridge，遵守架构规则"presentation 层只有
 * PresentationContractBridge.kt 可以依赖 Bridge"（tools/check_android_architecture.py）。
 * 解析失败的角色返回 null（与旧 PresentationContractBridge.resolveScreenPolicy
 * 失败语义一致：对应 UI 区域按无契约处理，不崩溃）。
 *
 * 契约内容（哪些动作在哪些区域）的唯一事实来源是 Core screen_contract 及其
 * Rust 单测（screen_contract_tests.rs）；本类只负责"一次性解析 + 静态快照"。
 */
class PresentationPolicyCatalog(
    resolver: (ScreenRoleDto) -> ScreenPolicyDto?,
) {
    private val policies: Map<ScreenRoleDto, ScreenPolicyDto?> =
        ROLES.associateWith(resolver)

    operator fun get(role: ScreenRoleDto): ScreenPolicyDto? = policies[role]

    companion object {
        internal val ROLES: List<ScreenRoleDto> =
            listOf(
                ScreenRoleDto.PROJECT_LIST,
                ScreenRoleDto.PROJECT_WORKSPACE,
                ScreenRoleDto.WRITING,
                ScreenRoleDto.STAR_MAP,
                ScreenRoleDto.STATS,
                ScreenRoleDto.SETTINGS,
            )
    }
}
