import QtQuick 2.15

QtObject {
    id: controller

    property var backendRef: null
    property var appState: ({
        hasWorkspace: false,
        workspacePath: "",
        saveStatus: "",
        selected: { projectId: "", volumeId: "", chapterId: "" },
        tree: [],
        settings: { fontSize: 16, themeMode: "dark" },
        sync: { status: "not_configured" }
    })
    property string route: "hub"
    property string writingProjectId: ""
    property string writingProjectTitle: ""
    property string starmapId: ""
    property string starmapTitle: ""
    property string errorMessage: ""

    readonly property bool inWriting: route === "writing"
    readonly property bool inStarmap: route === "starmap"

    signal errorRaised(string message)
    signal stateChanged(var state)

    function generateActionId() {
        return Math.random().toString(36).substring(2, 8);
    }

    function emitError(message) {
        errorMessage = message || "操作失败";
        errorRaised(errorMessage);
    }

    function parseJson(raw, fallbackMessage) {
        try {
            return JSON.parse(raw);
        } catch (e) {
            emitError((fallbackMessage || "解析后端返回数据失败") + ": " + e);
            return null;
        }
    }

    function applyState(state) {
        if (!state) return;
        appState = state;
        stateChanged(appState);
    }

    function refreshState(fallbackMessage) {
        if (!backendRef) return;
        var state = parseJson(backendRef.refresh_app_state_json(), fallbackMessage || "刷新应用状态失败");
        if (state) applyState(state);
    }

    function restoreWorkspace() {
        if (!backendRef) return;
        backendRef.query_system_color_scheme();
        backendRef.try_restore_last_workspace();
        refreshState("恢复工作区失败");
    }

    function openWriting(projectId, projectTitle) {
        writingProjectId = projectId || "";
        writingProjectTitle = projectTitle || "作品";
        starmapId = "";
        starmapTitle = "";
        route = "writing";
    }

    function openStarmap(id, title) {
        starmapId = id || "";
        starmapTitle = title || "星图编辑器";
        writingProjectId = "";
        writingProjectTitle = "";
        route = "starmap";
    }

    function openHub() {
        route = "hub";
        writingProjectId = "";
        writingProjectTitle = "";
        starmapId = "";
        starmapTitle = "";
        refreshState("返回工作台失败");
    }

    function switchWorkspace() {
        if (!backendRef) return;
        backendRef.switch_workspace();
        route = "hub";
        refreshState("切换工作区失败");
    }

    function createWorkspace(openExisting) {
        if (!backendRef) return;
        if (openExisting) backendRef.open_existing_workspace();
        else backendRef.create_new_workspace();
        refreshState("打开工作区失败");
    }

    function handleMutationResult(raw, fallbackMessage) {
        var res = parseJson(raw, fallbackMessage || "操作失败");
        if (!res) return false;
        if (res.success) {
            if (res.state) applyState(res.state);
            else refreshState("刷新操作结果失败");
            return true;
        }
        emitError(res.userMessage || res.message || fallbackMessage || "操作失败");
        return false;
    }

    function createProject(title) {
        if (!backendRef) return false;
        var actionId = generateActionId();
        try {
            return handleMutationResult(backendRef.create_project_json(title, actionId), "创建作品失败");
        } catch (e) {
            emitError("后端调用失败: " + e);
            return false;
        }
    }

    function renameProject(projectId, title) {
        if (!backendRef || !projectId || !title) return false;
        try {
            backendRef.rename_project(projectId, title);
            refreshState("刷新重命名结果失败");
            return true;
        } catch (e) {
            emitError("后端调用失败: " + e);
            return false;
        }
    }

    function createVolume(projectId, title) {
        if (!backendRef) return false;
        var actionId = generateActionId();
        try {
            return handleMutationResult(backendRef.create_volume_json(projectId, title, actionId), "创建卷失败");
        } catch (e) {
            emitError("后端调用失败: " + e);
            return false;
        }
    }

    function createChapter(projectId, volumeId, title) {
        if (!backendRef) return false;
        var actionId = generateActionId();
        try {
            return handleMutationResult(backendRef.create_chapter_json(projectId, volumeId, title, actionId), "创建章节失败");
        } catch (e) {
            emitError("后端调用失败: " + e);
            return false;
        }
    }

    function deleteItem(type, contextData) {
        if (!backendRef) return false;
        var actionId = generateActionId();
        var raw = "";
        try {
            if (type === "delete_project") raw = backendRef.delete_project_json(contextData.projectId, actionId);
            else if (type === "delete_volume") raw = backendRef.delete_volume_json(contextData.projectId, contextData.volumeId, actionId);
            else if (type === "delete_chapter") raw = backendRef.delete_chapter_json(contextData.projectId, contextData.volumeId, contextData.chapterId, actionId);
            else return false;
            return handleMutationResult(raw, "删除失败");
        } catch (e) {
            emitError("后端调用失败: " + e);
            return false;
        }
    }
}
