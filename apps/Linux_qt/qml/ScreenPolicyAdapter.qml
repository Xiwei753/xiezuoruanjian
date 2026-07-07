// =============================================================================
// ScreenPolicyAdapter.qml — 从 Core 获取页面动作位置语义
// =============================================================================
//
// 层级：Linux_qt UI 层（QML helper 组件）
// 职责：调用 AppBackend.resolve_screen_policy 获取 ActionSlot 列表，
//       暴露便捷查询方法给 QML 页面使用
// 约束：
//   - 只绑定数据，不写业务逻辑
//   - 所有位置语义来自 Core，不硬编码
//   - 通过 backendRef 或 appBackend 调用 Rust 侧方法
//
// 用法：
//   ScreenPolicyAdapter {
//     id: screenPolicy
//     screenRole: "Writing"
//     shellMode: root.layoutPlan ? root.layoutPlan.shellMode : "SinglePane"
//   }
//
//   screenPolicy.slotForRole("Back")          // 获取某个动作的 slot 信息
//   screenPolicy.isRoleAtPlacement("Back", "TopLeading")  // 判断位置
//   screenPolicy.isRoleVisible("Back")         // 判断可见性
// =============================================================================

import QtQuick 2.15

QtObject {
    id: root

    // ── 输入属性 ──
    property var backendRef: null
    property string screenRole: "Writing"
    property string shellMode: "SinglePane"

    // ── 输出：actionSlots 列表 ──
    property var actionSlots: []

    // 当输入变化时重新获取策略
    onScreenRoleChanged: refresh()
    onShellModeChanged: refresh()

    function refresh() {
        // 优先使用 backendRef，回退到全局 appBackend
        var backend = root.backendRef ? root.backendRef :
                      (typeof appBackend !== 'undefined' ? appBackend : null)
        if (!backend || !backend.resolve_screen_policy) return
        try {
            var result = backend.resolve_screen_policy(screenRole, shellMode)
            if (result && result.actionSlots !== undefined) {
                root.actionSlots = result.actionSlots
            } else {
                root.actionSlots = []
            }
        } catch (e) {
            console.warn("ScreenPolicyAdapter: resolve_screen_policy failed:", e)
            root.actionSlots = []
        }
    }

    // ── 便捷方法：查找某个 ActionRole 的 slot ──
    function slotForRole(role) {
        for (var i = 0; i < actionSlots.length; i++) {
            if (actionSlots[i].role === role) {
                return actionSlots[i]
            }
        }
        return null
    }

    // ── 便捷方法：判断某个 ActionRole 是否在指定 placement ──
    function isRoleAtPlacement(role, placement) {
        var slot = slotForRole(role)
        return slot !== null && slot.placement === placement
    }

    // ── 便捷方法：判断某个 ActionRole 是否可见（存在于当前 shellMode 的 slot 列表中）──
    function isRoleVisible(role) {
        var slot = slotForRole(role)
        if (slot === null) return false
        // 检查 visibleIn 是否包含当前 shellMode
        if (slot.visibleIn && Array.isArray(slot.visibleIn)) {
            return slot.visibleIn.indexOf(root.shellMode) >= 0
        }
        // 如果没有 visibleIn 字段，默认可见
        return true
    }

    Component.onCompleted: refresh()
}