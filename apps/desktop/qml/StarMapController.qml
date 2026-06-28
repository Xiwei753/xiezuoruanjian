// =============================================================================
// StarMapController.qml — 星图列表操作控制器
// =============================================================================

import QtQuick

QtObject {
    id: controller

    property var backendRef: null
    property var starmapBackendRef: null
    property var appController: null

    function starmapApi() {
        return starmapBackendRef || backendRef;
    }

    function listStarmaps() {
        var api = starmapApi();
        if (!api || !appController) return [];
        try {
            var res = appController.parseJson(api.list_starmaps_json(), qsTr("加载星图列表失败"));
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
            return appController.handleMutationResult(api.create_starmap_json(title, description || "", ""), qsTr("创建星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createChildStarmap(parentId, title, description) {
        var api = starmapApi();
        if (!api || !appController || !parentId || !title) return false;
        try {
            return appController.handleMutationResult(api.create_child_starmap_json(parentId, title, description || "", ""), qsTr("创建子星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameStarmap(starmapId, title) {
        var api = starmapApi();
        if (!api || !appController || !starmapId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_starmap_json(starmapId, title), qsTr("重命名星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function deleteStarmap(starmapId) {
        var api = starmapApi();
        if (!api || !appController || !starmapId) return false;
        try {
            return appController.handleMutationResult(api.delete_starmap_json(starmapId), qsTr("删除星图失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }
}
