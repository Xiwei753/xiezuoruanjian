// =============================================================================
// StarMapSelectionController.qml — 星图递归树共享选中状态
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 逻辑控制器）
// 职责：整个根星图递归树共享的唯一选中状态
//
// Issue #814 评论 5935285879：不再让每层 GraphController 各自保存一份 isSelected。
// 整棵递归树（根 Scene + 各层 Embed 内部 child Scene）共用一个
// StarMapSelectionController 实例，由 Workspace 创建并逐层下传。
//
// 选中身份由三元组 (scenePathKey, kind, itemId) 唯一确定：
//   - scenePathKey：选中项所在 Scene 的 pathKey（区分根层与各层子 Scene）
//   - kind：选中项类型（"node" / "embed" / "edge"）
//   - itemId：选中项在该 Scene 内的 id（node.id / embed.instanceId / edge.id）
//
// Node / Embed / Edge 的 isSelected 全部从 matches(pathKey, kind, id) 派生，
// GraphController 不再用 applySelection 数组重建维护 isSelected。
//
// 约束：
//   - 纯状态管理，不读写 Core
//   - 整棵递归树只有一个实例，子 Scene 沿用同一个，不每层新建
// =============================================================================

import QtQuick

QtObject {
    // 当前选中项所在 Scene 的 pathKey（"" 表示无选中）
    property string scenePathKey: ""

    // 当前选中项类型："" / "node" / "embed" / "edge"
    property string kind: ""

    // 当前选中项在该 Scene 内的 id
    property string itemId: ""

    // 选中指定项。调用方负责传当前 Scene 的 pathKey。
    function select(pathKey, nextKind, nextItemId) {
        scenePathKey = pathKey
        kind = nextKind
        itemId = nextItemId
    }

    // 清空选中。
    function clear() {
        scenePathKey = ""
        kind = ""
        itemId = ""
    }

    // 判断指定项是否为当前选中项。Node/Embed/Edge 的 isSelected 派生自此。
    function matches(pathKey, nextKind, nextItemId) {
        return scenePathKey === pathKey && kind === nextKind && itemId === nextItemId
    }
}
