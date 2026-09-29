// =============================================================================
// StarMapController.qml — 星图列表操作控制器
// =============================================================================

import QtQuick

QtObject {
    id: controller

    property var starmapBackendRef: null
    property var appController: null

    function starmapApi() {
        return starmapBackendRef;
    }

    // Issue #788 评论 5868510015：Rust 侧 create/rename/delete_starmap_json()
    // 返回的是 JSON envelope 字符串（QString），而 AppController.handleMutationResult()
    // 期望已解析的 JS 对象（直接读 res.success）。若把原始字符串传进去，res.success 恒为
    // undefined，成功写操作也会走失败分支。这里统一先 parseJson 解 envelope 再交给
    // handleMutationResult，与 listStarmaps 的处理方式保持一致。
    function handleMutationJson(raw, fallbackMessage) {
        var res = appController.parseJson(raw, fallbackMessage);
        if (!res) return false;
        return appController.handleMutationResult(res, fallbackMessage);
    }

    // Issue #796 评论 5886483653: 一级页只列根星图，调 list_root_starmaps_json。
    // 后端返回过滤掉子星图后的根星图列表；父星图里的 Embed 负责进入子星图，
    // 子星图不再和根星图并排出现在这里。
    function listStarmaps() {
        var api = starmapApi();
        if (!api || !appController) return [];
        try {
            var res = appController.parseJson(api.list_root_starmaps_json(), qsTr("加载星图列表失败"));
            if (!res) return [];
            if (res.success) return res.data || [];
            appController.emitError(qsTr("加载星图列表失败"));
            return [];
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return [];
        }
    }

    function createStarmap(title, description) {
        var api = starmapApi();
        if (!api || !appController || !title) return false;
        try {
            return handleMutationJson(api.create_starmap_json(title, description || "", ""), qsTr("创建星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameStarmap(starmapId, title) {
        var api = starmapApi();
        if (!api || !appController || !starmapId || !title) return false;
        try {
            return handleMutationJson(api.rename_starmap_json(starmapId, title), qsTr("重命名星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function deleteStarmap(starmapId) {
        var api = starmapApi();
        if (!api || !appController || !starmapId) return false;
        try {
            return handleMutationJson(api.delete_starmap_json(starmapId), qsTr("删除星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }
}
