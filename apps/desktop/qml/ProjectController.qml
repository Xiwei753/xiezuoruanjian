// =============================================================================
// ProjectController.qml — 项目树操作控制器
// =============================================================================

import QtQuick

QtObject {
    id: controller

    property var backendRef: null
    property var projectBackendRef: null
    property var appController: null

    function projectApi() {
        return projectBackendRef || backendRef;
    }

    function createProject(title) {
        var api = projectApi();
        if (!api || !appController) return false;
        try {
            return appController.handleMutationResult(api.create_project(title, appController.generateActionId()), qsTr("创建作品失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameProject(projectId, title) {
        var api = projectApi();
        if (!api || !appController || !projectId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_project(projectId, title), qsTr("重命名作品失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameVolume(projectId, volumeId, title) {
        var api = projectApi();
        if (!api || !appController || !projectId || !volumeId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_volume(projectId, volumeId, title), qsTr("重命名卷失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameChapter(projectId, volumeId, chapterId, title) {
        var api = projectApi();
        if (!api || !appController || !projectId || !volumeId || !chapterId || !title) return false;
        try {
            return appController.handleMutationResult(api.rename_chapter(projectId, volumeId, chapterId, title), qsTr("重命名章节失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createVolume(projectId, title) {
        var api = projectApi();
        if (!api || !appController) return false;
        try {
            return appController.handleMutationResult(api.create_volume(projectId, title, appController.generateActionId()), qsTr("创建卷失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createChapter(projectId, volumeId, title) {
        var api = projectApi();
        if (!api || !appController) return false;
        try {
            return appController.handleMutationResult(api.create_chapter(projectId, volumeId, title, appController.generateActionId()), qsTr("创建章节失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function deleteItem(type, contextData) {
        var api = projectApi();
        if (!api || !appController) return false;
        var actionId = appController.generateActionId();
        var resObj = null;
        try {
            if (type === "delete_project") resObj = api.delete_project_result(contextData.projectId, actionId);
            else if (type === "delete_volume") resObj = api.delete_volume_result(contextData.projectId, contextData.volumeId, actionId);
            else if (type === "delete_chapter") resObj = api.delete_chapter_result(contextData.projectId, contextData.volumeId, contextData.chapterId, actionId);
            else return false;
            return appController.handleMutationResult(resObj, qsTr("删除失败"));
        } catch (e) {
            appController.emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }
}
