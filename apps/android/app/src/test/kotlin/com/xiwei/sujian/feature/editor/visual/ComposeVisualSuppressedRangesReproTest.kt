package com.xiwei.sujian.feature.editor.visual

import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #689 评论 5674631257：持续视觉状态重构。
 *
 * 本测试文件原断言旧事务机制（masterProgress 归零、startFrame 物化、activeTransaction、
 * suppressedCurrentRanges 继承、textAnimationActive/cursorAnimationActive 冻结字段等）。
 * 旧机制已被完全删除（不是修改），断言对象已不存在。
 *
 * 新持续 timeline 的行为由 [ComposeVisualTransactionRestartReproTest] 验证：
 * - 快速输入时已有 unit 的 alpha 通道 startedAtNanos 不被重置
 * - 新事务不把 masterProgress 归零（因为已不存在）
 * - 删换行时几何没变的存活 unit 不产生 position track
 * - hiddenRanges 从当前 overlay unit 推导而非继承
 * - patch 不携带旧事务状态（startFrame / suppressedCurrentRanges 等）
 *
 * 旧测试方法已删除。如需在新模型下重写等价测试，参见
 * [ComposeVisualTransactionRestartReproTest] 的测试模式。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualSuppressedRangesReproTest {
    /**
     * 占位测试 — 确保本类编译通过。
     * 旧事务机制已删除，本测试文件的历史测试方法已移除。
     */
    @Test
    fun placeholder_oldTransactionMechanismRemoved() {
        // 旧事务机制（masterProgress / startFrame / activeTransaction / suppressedCurrentRanges /
        // textAnimationActive / cursorAnimationActive / reportProgress / finishTransaction）
        // 已由 #689 评论 5674631257 持续视觉状态重构删除。
        // 新持续 timeline 行为由 ComposeVisualTransactionRestartReproTest 验证。
    }
}
