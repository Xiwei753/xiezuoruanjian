package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一：窗口层不持有窗口坐标追踪 mutableState 契约测试。
 *
 * Issue #595 要求删除根壳全屏覆盖层、窗口坐标追踪和正文用全局 AnimatedTextEditorSlot。
 * 根壳覆盖层删除后，[EditorWindowHost] 不再需要窗口级 activeTargetGeometry/
 * activeTargetTransform 缓存（它们曾用于 graphicsLayer 平移覆盖层到正文区域）。
 * 本测试锁定该不变量，防止回归。
 *
 * 短文本 target（项目标题、搜索框、星图标签）仍通过 [EditableTextTarget.currentGeometry]
 * 持有自己的几何（由 updateTargetGeometry 更新），但窗口层不再缓存活动 target 的几何
 * 到 mutableState 字段。
 */
class WindowLayerNoCoordinateTrackingContractTest {

    @Test
    fun editorWindowHost_doesNotHoldActiveTargetGeometryMutableState() {
        val offendingFields = EditorWindowHost::class.java.declaredFields.filter { field ->
            field.name.contains("activeTargetGeometry", ignoreCase = true) ||
                field.name.contains("activeTargetTransform", ignoreCase = true)
        }

        assertTrue(
            "EditorWindowHost must not hold activeTargetGeometry/activeTargetTransform " +
            "after #595 一 (root overlay removed). Found: " +
            offendingFields.map { it.name + ": " + it.type.name },
            offendingFields.isEmpty()
        )
    }

    @Test
    fun editorWindowHost_doesNotUseComposeMutableStateForCoordinateTracking() {
        val composeMutableStateTypes = setOf(
            "androidx.compose.runtime.MutableState",
            "androidx.compose.runtime.MutableIntState",
            "androidx.compose.runtime.MutableLongState",
            "androidx.compose.runtime.MutableFloatState",
            "androidx.compose.runtime.MutableDoubleState",
        )

        val offendingFields = EditorWindowHost::class.java.declaredFields.filter { field ->
            composeMutableStateTypes.any { it == field.type.name }
        }

        assertTrue(
            "EditorWindowHost must not hold Compose mutableState fields for coordinate " +
            "tracking after #595 一. Found: " +
            offendingFields.map { it.name + ": " + it.type.name },
            offendingFields.isEmpty()
        )
    }

    @Test
    fun editorWindowHost_stillExposesTargetGeometryApiForShortTextTargets() {
        val methods = EditorWindowHost::class.java.methods
        val updateGeometry = methods.firstOrNull { it.name == "updateTargetGeometry" }
        val updateTransform = methods.firstOrNull { it.name == "updateTargetTransform" }
        val getGeometry = methods.firstOrNull { it.name == "getTargetGeometry" }

        assertTrue(
            "EditorWindowHost must still expose updateTargetGeometry for short-text targets " +
            "(project title, search, starmap) after #595 一",
            updateGeometry != null
        )
        assertTrue(
            "EditorWindowHost must still expose updateTargetTransform for short-text targets " +
            "after #595 一",
            updateTransform != null
        )
        assertTrue(
            "EditorWindowHost must still expose getTargetGeometry for short-text targets " +
            "after #595 一",
            getGeometry != null
        )
    }
}
