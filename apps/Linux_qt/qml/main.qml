// =============================================================================
// main.qml — 应用入口窗口
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：应用主窗口、全局路由管理、初始化 AppBackend、全局错误处理
// 约束：
//   - 不包含任何业务逻辑，所有操作委托给 AppBackend (Rust QObject)
//   - 页面切换通过 appController.route 状态驱动
//   - 不直接操作文件系统或 Core 层
//
// 调用链：main.qml → AppBackend (Rust QObject) → WriterCore (Core 层)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import QtQuick.Window
import Sujian 1.0

ApplicationWindow {
    id: window
    visible: true
    // Issue #692 评论 5692612221: 默认逻辑尺寸 1100×768，窗口真正关联到屏幕后
    // 用当前窗口所属屏幕的 QScreen::availableGeometry()（已扣 KDE 面板/任务栏等
    // 窗口管理器保留区域）做一次性初始尺寸收口。QML 的 window.screen
    // （QQuickScreenInfo）只暴露 width/height（完整几何）和
    // desktopAvailableWidth/Height（虚拟桌面可用），都不合适：前者含任务栏，
    // 后者是所有屏幕合起来的可用尺寸，双屏混合 DPI 时限制等于没限制。
    // 因此通过 appBackend.available_screen_geometry_json() 桥接 Qt C++
    // QWindow::screen()->availableGeometry()，拿到当前屏幕扣保留区域后的可用
    // 尺寸做上限收口。窗口跨屏后的 DPR / fractional scaling 完全交给 Qt，
    // 不手工重算。
    width: 1100
    height: 768
    title: qsTr("素笺写作")
    color: designTokens.bg

    // ── Issue #692 评论 5692612221: 初始窗口尺寸收口（一次性） ──
    // 等窗口真正关联到所属屏幕后，按当前屏幕 availableGeometry 收口默认尺寸。
    // initialWindowSizeApplied 之后不再跟随多屏切换强制改窗口大小；窗口跨屏后的
    // DPR / fractional scaling 完全交给 Qt。
    property bool initialWindowSizeApplied: false

    function applyInitialWindowSize() {
        if (initialWindowSizeApplied)
            return
        if (appBackend === null)
            return

        var geomJson = appBackend.available_screen_geometry_json()
        var geom = null
        try {
            geom = JSON.parse(geomJson)
        } catch (e) {
            geom = null
        }
        if (!geom || !geom.valid)
            return

        var aw = geom.width
        var ah = geom.height
        if (aw <= 0 || ah <= 0)
            return

        window.width = Math.min(1100, Math.max(320, aw))
        window.height = Math.min(768, Math.max(240, ah))
        initialWindowSizeApplied = true
    }

    onVisibleChanged: {
        if (visible && !initialWindowSizeApplied)
            Qt.callLater(applyInitialWindowSize)
    }

    onScreenChanged: {
        if (!initialWindowSizeApplied)
            Qt.callLater(applyInitialWindowSize)
    }

    // ── Material 主题绑定：确保 Qt 原生控件（Dialog/Popup/Menu/TextField 等）跟随深浅色 ──
    Material.theme: designTokens.isDark ? Material.Dark : Material.Light
    Material.primary: designTokens.primary
    Material.accent: designTokens.primary
    Material.foreground: designTokens.textPrimary
    Material.background: designTokens.bg

    // ── Palette 绑定：保留作为非 Material 控件的 fallback ──
    palette.window: designTokens.bg
    palette.windowText: designTokens.textPrimary
    palette.base: designTokens.surfaceContainerLow
    palette.text: designTokens.textPrimary
    palette.button: designTokens.surfaceContainer
    palette.buttonText: designTokens.textPrimary
    palette.highlight: designTokens.primary
    palette.highlightedText: designTokens.onPrimary

    function reportNullBackend(name) {
        var message = "Required QML context property is null: " + name;
        console.error(message);
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("error", "app", "null_context_property", message);
        }
    }

    function verifyBackendRuntime() {
        if (appBackend === null) reportNullBackend("appBackend");
        if (workspaceBackend === null) reportNullBackend("workspaceBackend");
        if (projectBackend === null) reportNullBackend("projectBackend");
        if (editorBackend === null) reportNullBackend("editorBackend");
        if (settingsBackend === null) reportNullBackend("settingsBackend");
        if (syncBackend === null) reportNullBackend("syncBackend");
        if (starmapBackend === null) reportNullBackend("starmapBackend");
    }

    function debugLog(module, event, message) {
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("info", module, event, message);
        }
    }

    function debugWarn(module, event, message) {
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("warn", module, event, message);
        }
    }

    function debugError(module, event, message) {
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("error", module, event, message);
        }
    }

    function preSyncBarrier() {
        if (appController.inWriting && writingWorkspaceLoader.item) {
            if (!writingWorkspaceLoader.item.flushActiveEditorBeforeSync()) return false
        }
        if (editorBackend) {
            editorBackend.flush_writing_stats()
            editorBackend.flush_recent_edits()
        }
        if (settingsBackend) {
            settingsBackend.flush_pending_settings_save()
        }
        return true
    }

    function openSettingsDialog() {
        if (!settingsDialogLoader.active) {
            settingsDialogLoader.active = true;
        }
        if (settingsDialogLoader.item) {
            settingsDialogLoader.item.open();
        }
    }

    // Issue #762 评论 5826175490 第 4 点：从 SyncPage 全局冲突入口跳到具体作品。
    // 待选中的冲突路径先记在这里，等 WritingWorkspace 实例化后再交给它——
    // 用户可能在 hub/设置页触发，此时 writingWorkspaceLoader.item 还不存在。
    property string pendingConflictPath: ""

    function projectTitleById(projectId) {
        var treeData = appState ? (appState.tree || []) : []
        for (var i = 0; i < treeData.length; i++) {
            if (treeData[i].type === "project" && treeData[i].id === projectId) {
                return treeData[i].title || ""
            }
        }
        return ""
    }

    function applyPendingConflictPath() {
        if (!pendingConflictPath) return
        var workspace = writingWorkspaceLoader.item
        if (!workspace) return
        workspace.openConflictPath(pendingConflictPath)
        pendingConflictPath = ""
    }

    function openConflictInProject(projectId, path) {
        if (!projectId) return
        // 关闭设置页，让写作工作区可见。
        if (settingsDialogLoader.item) settingsDialogLoader.item.close()
        appController.openWriting(projectId, window.projectTitleById(projectId))
        window.pendingConflictPath = path || ""
        // 不要求这一轮同步先结束；打开作品与同步是否在跑互不影响。
        window.applyPendingConflictPath()
    }

    function openSyncDialog() {
        // Issue #696 评论 5698002089: 不在此处 load_local_settings()。
        // 打开设置/同步界面只负责开窗；本地主题设置只能由工作区初始化
        // internal_open_data_root() -> load_local_settings() 或无工作区的
        // load_app_theme_mode() 加载。UI 打开动作重新加载全局本地设置会触发
        // settings_changed -> themeController.reload()，导致主题异常切换。
        // 同步配置刷新由 SettingsDialog.onOpened 的 syncBackendRef.load_sync_config() 负责。
        window.openSettingsDialog();
    }


    property alias appState: appController.appState
    readonly property bool rootHasWorkspace: workspaceBackend !== null && workspaceBackend.has_workspace === true

    property string previousEditorText: ""

    // ── 布局契约驱动（#610）：Qt 侧按本平台窗口系统算能力，再套 Core 契约 ──
    property var layoutPlan: null

    function applyLayoutPlan() {
        if (appBackend === null) return;
        var w = window.width;
        var h = window.height;
        var plan = appBackend.resolve_layout(w, h);
        if (plan) {
            window.layoutPlan = plan;
        }
    }

    onWidthChanged: window.applyLayoutPlan()
    onHeightChanged: window.applyLayoutPlan()

    SystemPalette {
        id: systemPalette
        colorGroup: SystemPalette.Active
    }

    function colorLuminance(colorValue) {
        return 0.2126 * colorValue.r + 0.7152 * colorValue.g + 0.0722 * colorValue.b;
    }

    function systemPaletteIsDark() {
        return colorLuminance(systemPalette.window) < colorLuminance(systemPalette.windowText);
    }

    function systemThemeIsDark() {
        // Try Qt.styleHints (Qt 6.5+)
        if (typeof Qt !== "undefined" && Qt.styleHints && typeof Qt.styleHints.colorScheme !== "undefined") {
            var scheme = Qt.styleHints.colorScheme;
            if (scheme === 2 || (Qt.ColorScheme && scheme === Qt.ColorScheme.Dark)) {
                return true;
            }
            if (scheme === 1 || (Qt.ColorScheme && scheme === Qt.ColorScheme.Light)) {
                return false;
            }
        }
        // Try Qt.application.styleHints
        if (typeof Qt !== "undefined" && Qt.application && Qt.application.styleHints && typeof Qt.application.styleHints.colorScheme !== "undefined") {
            var schemeApp = Qt.application.styleHints.colorScheme;
            if (schemeApp === 2 || (Qt.ColorScheme && schemeApp === Qt.ColorScheme.Dark)) {
                return true;
            }
            if (schemeApp === 1 || (Qt.ColorScheme && schemeApp === Qt.ColorScheme.Light)) {
                return false;
            }
        }
        // Fallback to SystemPalette brightness inference
        return systemPaletteIsDark();
    }

    function logThemeDiagnostics(event, snapshot) {
        // Issue #709 评论 issue-body-709: 主题诊断只读 ThemeController runtime state
        // 和 designTokens，不再读旧 appState.settings.themeMode、Qt.styleHints.colorScheme、
        // systemPaletteIsDark()。这些不再是主题诊断的权威来源。同一条诊断写出
        // appearance_mode/is_dark/color_source/选中 theme/palette id/最终
        // primary/surface/on_surface/on_surface_variant/editorText。
        // Issue #736 评论 5778543593 修改2: 当传入 snapshot 时（onThemeApplied 调用），
        // 直接用 snapshot 的字段记录诊断，不经过第二轮 QML binding（Qt 不保证 binding
        // 求值顺序，诊断可能读到上一份值）。未传入 snapshot 时（其他调用点如
        // "system_color_scheme_changed"/"startup"），继续用 designTokens 派生 property。
        if (appBackend === null || !appBackend.log_qml) return;
        var tc = themeController;
        var primaryVal, surfaceVal, onSurfaceVal, onSurfaceVariantVal, editorTextVal;
        if (snapshot) {
            primaryVal = snapshot.primary;
            surfaceVal = snapshot.surface;
            onSurfaceVal = snapshot.on_surface;
            onSurfaceVariantVal = snapshot.on_surface_variant;
            editorTextVal = snapshot.editor_text;
        } else {
            primaryVal = designTokens.primary;
            surfaceVal = designTokens.surface;
            onSurfaceVal = designTokens.onSurface;
            onSurfaceVariantVal = designTokens.onSurfaceVariant;
            editorTextVal = designTokens.editorText;
        }
        appBackend.log_qml("info", "theme", event,
                        "appearance_mode=" + (tc ? tc.appearance_mode : "<null>")
                        + " is_dark=" + (tc ? tc.is_dark : "<null>")
                        + " color_source=" + (tc ? tc.color_source : "<null>")
                        + " builtin_theme_id=" + (tc ? tc.selected_builtin_theme_id : "<null>")
                        + " palette_id=" + (tc ? tc.selected_palette_id : "<null>")
                        + " primary=" + primaryVal
                        + " surface=" + surfaceVal
                        + " on_surface=" + onSurfaceVal
                        + " on_surface_variant=" + onSurfaceVariantVal
                        + " editorText=" + editorTextVal);
    }

    // Design tokens
    DesignTokens {
        id: designTokens
        // Issue #721: 颜色直接读 themeControllerRef.*_hex，不再从 themeStateJson 解析。
        // Issue #721 评论 5747140241: 属性名用 themeControllerRef，右侧 themeController 是
        // QQmlContext 注入对象。若属性名也叫 themeController，QML 绑定作用域（接收对象自身）
        // 会让右侧裸 themeController 遮蔽 QQmlContext 注入对象，自绑定保持 null，颜色链全走
        // fallback。属性名与注入名不同名后右侧明确拿外部注入对象。
        // themeStateJson 保留做 DesignTokens 输入，诊断改监听 themeApplied 信号，不参与颜色计算。
        themeControllerRef: themeController
        // Issue #702: 根 DesignTokens 只绑定 themeStateJson 这一份完整主题状态。
        // themeController 把 is_dark 和最终 ThemeColorScheme 打包成
        // {"is_dark": bool, "scheme": <object>} 一次性发布，QML 侧从同一份
        // JSON 解析 isDark 和 scheme，彻底消除 isDark 已是 true 但 scheme
        // 还是上一套浅色值的中间状态。不再分开绑定 is_dark 和
        // resolved_scheme_json 两个可能不同步的属性。
        themeStateJson: themeController !== null ? themeController.theme_state_json : ""
    }

    Connections {
        target: designTokens
        function onIsDarkChanged() {
            // Issue #668 评论 5646458592 问题 2: 删除 effective isDark 反写
            // themeController.set_system_is_dark 的逻辑。isDark 现在直接读
            // themeController.is_dark，反写会形成循环依赖。system_is_dark
            // 只由真实系统 colorScheme 变化（onColorSchemeChanged）和启动时
            // （Component.onCompleted）写入。
            if (appBackend) {
                appBackend.apply_window_dark_mode(designTokens.isDark);
            }
        }
        // Issue #736 评论 5777408243 问题2 / 5778543593 修改2: 主题诊断改为监听
        // DesignTokens 的 themeApplied 信号。themeApplied 在 applyThemeState() 完成
        // resolvedTheme 整体替换后发出，携带这份已发布的最终 token snapshot。
        // Issue #736 评论 5778543593 修改2: onThemeApplied(snapshot) 直接用 snapshot
        // 记录诊断，不经过第二轮 QML binding（Qt 不保证 binding 求值顺序，诊断可能
        // 读到上一份值）。
        function onThemeApplied(snapshot) {
            window.logThemeDiagnostics("theme_applied", snapshot);
        }
    }

    Connections {
        target: syncBackend
        function onSync_action_completed() {
            if (settingsBackend) settingsBackend.refresh_theme_data()
            // Issue #724 评论 5750911834 问题 3: 改用 reload_from_backend_if_changed()
            // 避免重复 resolve。同步完成后主题输入可能未变，跳过 resolve。
            if (themeController) themeController.reload_from_backend_if_changed()
        }
        // Issue #754 评论 5814866116 改动1: 同步真正修改/重新加载了当前工作区内容时，
        // 由 SyncBackend::sync_content_applied 触发正文/树刷新。
        // onSync_action_completed 只处理同步状态/主题相关刷新，不无条件重载正文。
        // Issue #754 评论 5815901258: 用 refreshStateImmediate 立即拿到最新 appState，
        // 再用 reconcileActiveChapter 以权威 appState.selected 收口编辑器——同步删除当前
        // 章节/分卷/作品时编辑器退回"请选择或新建章节"，不再残留已删除正文。
        function onSync_content_applied() {
            appController.refreshStateImmediate(qsTr("刷新工作区内容失败"));

            if (appController.inWriting && writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.reconcileActiveChapter(
                    appController.appState && appController.appState.selected
                        ? appController.appState.selected
                        : null
                );
            }
        }
    }

    // Issue #668 评论 5646458592 问题 2: 真实系统 colorScheme 变化时把
    // window.systemThemeIsDark() 写给 ThemeController。这是 system_is_dark
    // 的两个唯一写入点之一（另一个是 Component.onCompleted 启动时）。
    // 不再由 designTokens.onIsDarkChanged 反写 system_is_dark，避免循环依赖。
    Connections {
        target: Qt.styleHints
        function onColorSchemeChanged() {
            if (themeController) {
                themeController.set_system_is_dark(window.systemThemeIsDark());
            }
            window.logThemeDiagnostics("system_color_scheme_changed");
        }
    }

    AppController {
        id: appController
        workspaceBackendRef: workspaceBackend
        projectBackendRef: projectBackend
        appBackendRef: appBackend
        onErrorRaised: function(message) {
            errorDialog.message = message;
            errorDialog.open();
        }
    }

    ProjectController {
        id: projectController
        projectBackendRef: projectBackend
        appController: appController
    }

    StarMapController {
        id: globalStarMapController
        starmapBackendRef: starmapBackend
        appController: appController
    }

    Component.onCompleted: {
        // 启动守卫：确保 DesignTokens 已初始化
        if (!designTokens) {
            console.error("[STARTUP GUARD] designTokens is null!")
        }
        window.verifyBackendRuntime();
        window.debugLog("app", "qml_completed", "QML components fully loaded");
        // Issue #668 评论 5646458592 问题 2: 启动时把真实系统 colorScheme
        // 写给 ThemeController，作为 system_is_dark 的唯一写入点之一。
        // themeController 内部根据 appearance_mode（dark/light/system）决定
        // 是否使用 system_is_dark，QML 不再自算 isDark。
        if (themeController) {
            themeController.set_system_is_dark(window.systemThemeIsDark());
        }
        window.logThemeDiagnostics("startup");
        if (appBackend) {
            appBackend.apply_window_dark_mode(designTokens.isDark);
        }
        appController.restoreWorkspace();
        // Issue #696 评论 5696993601: 无工作区时 load_app_theme_mode() 已在
        // try_restore_last_workspace 内部执行完毕，这里发布一次
        // themeController.reload() 作为无工作区 fallback 路径的主题收口点。
        // 有工作区时由 onWorkspace_opened 收口，此处不重复触发。
        Qt.callLater(function() {
            if (!window.rootHasWorkspace && themeController) {
                themeController.reload();
            }
        });
    }

    onClosing: {
        if (appController.inWriting) {
            editorBackend.flush_writing_stats();
            editorBackend.flush_recent_edits();
        }
        // 应用关闭前 flush pending settings save
        if (settingsBackend) {
            settingsBackend.flush_pending_settings_save();
        }
    }

    // ── Debounced settings save Timer ──
    // settingsBackend.save_requested 信号触发后，延迟 300ms 执行实际保存
    Timer {
        id: settingsDebounceSaveTimer
        interval: 300
        repeat: false
        onTriggered: {
            if (settingsBackend) {
                settingsBackend.do_save_local_settings();
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSave_requested() {
            settingsDebounceSaveTimer.restart();
        }
    }

    function applyState(state) {
        appController.applyState(state);
    }

    Connections {
        target: projectBackend
        function onProjects_reloaded() {
            appController.refreshState(qsTr("刷新作品列表失败"));
        }
        function onSelected_item_changed() {
            appController.refreshStateImmediate(qsTr("刷新当前选择失败"))
            if (appController.inWriting)
                appController.saveNavigationState()
        }
    }

    Connections {
        target: workspaceBackend
        function onWorkspace_state_changed() {
            appController.refreshState(qsTr("刷新工作区状态失败"));
        }
        function onWorkspace_opened() {
            // Issue #696 评论 5696993601: 工作区恢复完成、
            // AppBackend::load_local_settings() 已执行后，立即调用一次
            // themeController.reload() 发布首次有效 scheme。不再等设置页
            // onOpened 重新 load_local_settings 触发 settings_changed 才切换。
            if (themeController) {
                themeController.reload();
            }
        }
        function onWorkspace_content_changed() {
            appController.refreshState(qsTr("刷新工作区内容失败"));
        }
    }

    Connections {
        target: editorBackend
        function onClear_editor() {
            if (writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.previousEditorText = "";
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSettings_changed() {
            appController.refreshState(qsTr("刷新设置失败"));
            // Issue #668 评论 5646458592 问题 2: 设置变更（如切换主题/配色/
            // appearance_mode）后调用 themeController.reload()，让
            // themeController 重新解析主题并发出 scheme_changed，使
            // DesignTokens.isDark / resolvedSchemeJson 等跟随更新。
            // 不再只 refreshState 而漏掉主题重载，导致 light/dark、isDark、
            // editorText 不同步。
            // Issue #724 评论 5750911834 问题 3: 改用 reload_from_backend_if_changed()
            // 避免重复 resolve。设置变更可能不涉及主题输入（如字体大小），
            // 此时跳过 resolve，避免 574 次 theme.resolve 和 light/dark 混合状态。
            if (themeController) {
                themeController.reload_from_backend_if_changed();
            }
        }
    }

    // === Main Content ===
    Item {
        anchors.fill: parent

        // StarMapWorkspace: shown when in starmap editor mode
        Loader {
            id: starmapWorkspaceLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.inStarmap
            sourceComponent: StarMapWorkspace {
                dt: designTokens
                starmapBackendRef: starmapBackend
                starmapId: appController.starmapId
                starmapTitle: appController.starmapTitle
                onBackClicked: {
                    appController.openHub();
                }
            }
        }

        // CreativeHub: shown when workspace open and not in writing mode
        Loader {
            id: creativeHubLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.route === "hub"
            sourceComponent: CreativeHub {
                dt: designTokens
                projectBackendRef: projectBackend
            editorBackendRef: editorBackend
                starmapBackendRef: starmapBackend
                starMapController: globalStarMapController
                appState: window.appState
                tree: window.appState.tree || []
                aiCapable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled
                layoutPlan: window.layoutPlan

                onOpenStarmapWorkspace: function(smId, smTitle) {
                    appController.openStarmap(smId, smTitle);
                }

                onOpenProject: function(projectId, projectTitle) {
                    appController.openWriting(projectId, projectTitle);
                    window.debugLog("workspace", "enter_writing_mode", "projectId=" + projectId);
                }

                onCreateProject: {
                    window.debugLog("project", "create_project_dialog_open", "");
                    createProjectDialog.open();
                }

                onRenameProjectRequested: function(projectId, title) {
                    projectController.renameProject(projectId, title);
                }

                onDeleteProjectRequested: function(projectId, title) {
                    confirmDialog.actionType = "delete_project";
                    confirmDialog.contextData = { projectId: projectId, title: title };
                    confirmDialog.open();
                }

                onOpenSettings: {
                    window.debugLog("settings", "settings_dialog_open", "");
                    window.openSettingsDialog();
                }

                onRequestSync: {
                    if (!window.preSyncBarrier()) return;
                    // 顶栏手动同步入口：基础配置可用时直接把请求交给 syncBackend。
                    // 不再用 !syncBackend.sync_in_progress 拦截——syncBackend 自己排队
                    // （manual_sync_pending）。
                    if (syncBackend) {
                        syncBackend.perform_sync();
                    }
                }

                onSwitchWorkspace: {
                    window.debugLog("workspace", "switch_workspace_clicked", "");
                    appController.switchWorkspace();
                }
            }
        }

        // WritingWorkspace: shown when in writing mode
        Loader {
            id: writingWorkspaceLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.inWriting
            // Issue #762 评论 5826175490 第 4 点：作品从全局冲突入口打开时，
            // Loader 到这里才有 item，补交待选中的冲突路径。
            onLoaded: window.applyPendingConflictPath()
            sourceComponent: WritingWorkspace {
                dt: designTokens
                projectBackendRef: projectBackend
                editorBackendRef: editorBackend
                starMapController: globalStarMapController
                // Issue #709 评论 issue-body-709: 传入 themeController 使
                // EditorController.logRenderColorProbe 能读取 runtime state。
                themeController: themeController
                appState: window.appState
                tree: window.appState.tree || []
                workspaceProjectId: appController.writingProjectId
                projectTitle: appController.writingProjectTitle
                aiCapable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled
                layoutPlan: window.layoutPlan
                // Issue #757 评论 5818193510 第 5 点：传入 syncBackend 给 WritingWorkspace，
                // 用于监听同步完成信号并在冲突产生时打开临时冲突侧栏。
                syncBackendRef: syncBackend

                onBackToProjects: {
                    appController.openHub();
                    window.debugLog("workspace", "exit_writing_mode", "");
                }

                onOpenSettings: {
                    window.openSettingsDialog();
                }


                onCreateVolumeRequested: function(projectId) {
                    inputDialog.actionType = "volume";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = "";
                    inputDialog.dialogTitle = qsTr("新建卷");
                    inputDialog.defaultText = "";
                    inputDialog.open();
                }

                onCreateChapterRequested: function(projectId, volumeId) {
                    inputDialog.actionType = "chapter";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = volumeId;
                    inputDialog.dialogTitle = qsTr("新建章节");
                    // 从树数据中统计当前卷的章节数量，生成默认标题
                    var chapterCount = 0;
                    var treeData = window.appState ? (window.appState.tree || []) : [];
                    for (var i = 0; i < treeData.length; i++) {
                        if (treeData[i].type === "chapter" && treeData[i].volumeId === volumeId) {
                            chapterCount++;
                        }
                    }
                    inputDialog.defaultText = qsTr("第%1章").arg(chapterCount + 1);
                    inputDialog.open();
                }

                onRenameItemRequested: function(itemData) {
                    inputDialog.actionType = "rename_" + itemData.type;
                    inputDialog.projectId = itemData.projectId || "";
                    inputDialog.volumeId = itemData.volumeId || "";
                    inputDialog.chapterId = itemData.id || "";
                    inputDialog.dialogTitle = qsTr("重命名");
                    inputDialog.defaultText = itemData.title || "";
                    inputDialog.open();
                }

                onDeleteItemRequested: function(itemData) {
                    confirmDialog.actionType = "delete_" + itemData.type;
                    confirmDialog.contextData = {
                        projectId: itemData.projectId || "",
                        volumeId: itemData.volumeId || "",
                        chapterId: itemData.id || "",
                        title: itemData.title || ""
                    };
                    confirmDialog.open();
                }
            }
        }

        // EmptyWorkspace: loaded only when no workspace (Loader destroys on deactivate)
        Loader {
            id: emptyWorkspaceLoader
            anchors.fill: parent
            active: !rootHasWorkspace
            onActiveChanged: {
                window.debugLog("app", "empty_workspace_loader_active_changed", "active=" + active);
            }
            sourceComponent: EmptyWorkspace {
                workspaceBackendRef: workspaceBackend
                dt: designTokens
                onCreateWorkspaceWithPath: (path) => {
                    appController.createWorkspaceWithPath(path, false);
                }
                onOpenWorkspaceWithPath: (path) => {
                    appController.createWorkspaceWithPath(path, true);
                }
                onInitFromGithub: {
                    window.openSyncDialog()
                }

            }
        }
    }

    // === Dialogs ===
    CreateProjectDialog {
        id: createProjectDialog
        theme: designTokens
        onSubmitProject: function(title) {
            var trimmedTitle = title ? title.trim() : "";
            var isEmpty = (trimmedTitle === "");
            window.debugLog("project", "create_project_submit", "titleLength=" + (title ? title.length : 0) + ", isEmpty=" + isEmpty);
            if (projectController.createProject(title)) {
                createProjectDialog.close();
            }
        }
    }

    Dialog {
        id: confirmDialog
        property string actionType: ""
        property var contextData: ({})

        title: qsTr("确认删除")
        modal: true
        width: 400
        height: 220
        parent: Overlay.overlay
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: designTokens.sp24
            spacing: designTokens.sp16

            AppText {
                dt: designTokens
                text: {
                    if (confirmDialog.actionType === "delete_project") return qsTr("您确定要删除作品「%1」及其所有分卷、章节吗？").arg(confirmDialog.contextData.title);
                    if (confirmDialog.actionType === "delete_volume") return qsTr("您确定要删除分卷「%1」及包含的所有章节吗？").arg(confirmDialog.contextData.title);
                    if (confirmDialog.actionType === "delete_chapter") return qsTr("您确定要删除章节「%1」吗？").arg(confirmDialog.contextData.title);
                    return qsTr("确定要删除吗？");
                }
                color: designTokens.textPrimary
                font.pointSize: designTokens.bodyPt
                font.family: designTokens.fontFamily
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignRight
                spacing: designTokens.sp8
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: designTokens
                    variant: "text"
                    onClicked: confirmDialog.close()
                }
                AppButton {
                    text: qsTr("删除")
                    dt: designTokens
                    variant: "danger"
                    onClicked: {
                        if (projectController.deleteItem(confirmDialog.actionType, confirmDialog.contextData)) {
                            confirmDialog.close();
                        }
                    }
                }
            }
        }
    }

    Dialog {
        id: errorDialog
        property string message: ""
        title: qsTr("提示")
        modal: true
        width: 340
        height: 180
        parent: Overlay.overlay
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: designTokens.sp24
            spacing: designTokens.sp16
            AppText {
                dt: designTokens
                text: errorDialog.message
                color: designTokens.textPrimary
                font.pointSize: designTokens.bodyPt
                font.family: designTokens.fontFamily
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("确定")
                dt: designTokens
                variant: "primary"
                Layout.alignment: Qt.AlignRight
                onClicked: errorDialog.close()
            }
        }
    }

    Loader {
        id: settingsDialogLoader
        active: false
        sourceComponent: SettingsDialog {
            theme: designTokens
            settingsBackendRef: settingsBackend
            workspaceBackendRef: workspaceBackend
            syncBackendRef: syncBackend
            editorBackendRef: editorBackend
            themeControllerRef: themeController
            beforeSyncHook: function() { return window.preSyncBarrier() }
            onSettingsChanged: {
                appController.refreshState(qsTr("刷新设置失败"));
                // Issue #724 评论 5750911834 问题 3: 改用 reload_from_backend_if_changed()
                // 避免与 settingsBackend.onSettings_changed 重复 resolve。
                if (themeController) themeController.reload_from_backend_if_changed();
            }
            // Issue #762 评论 5826175490 第 4 点：处理 SyncPage 的 openConflict 信号。
            // 打开对应作品并把目标冲突路径交给 WritingWorkspace/SyncConflictPanel 选中，
            // 不要求这一轮同步先结束。
            onOpenConflict: function(projectId, path) {
                window.openConflictInProject(projectId, path)
            }
        }
    }


    Dialog {
        id: inputDialog
        property string actionType: ""
        property string projectId: ""
        property string volumeId: ""
        property string chapterId: ""
        property string defaultText: ""
        property string dialogTitle: qsTr("请输入")

        modal: true
        width: 300
        height: 200
        parent: Overlay.overlay
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        title: inputDialog.dialogTitle

        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: designTokens.sp24
            spacing: designTokens.sp12

            AppText {
                dt: designTokens
                text: {
                    if (inputDialog.actionType === "volume") return qsTr("卷名称");
                    if (inputDialog.actionType === "chapter") return qsTr("章节名称");
                    return qsTr("新名称");
                }
                color: designTokens.textSecondary
                font.pointSize: designTokens.labelPt
                font.family: designTokens.fontFamily
            }

            AppTextField {
                id: inputField
                Layout.fillWidth: true
                dt: designTokens
                placeholderText: {
                    if (inputDialog.actionType === "volume") return qsTr("例如：第一卷");
                    if (inputDialog.actionType === "chapter") return qsTr("例如：第一章");
                    return qsTr("请输入新名称");
                }
                onAccepted: confirmInputButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    id: confirmInputButton
                    text: qsTr("确定")
                    dt: designTokens
                    variant: "primary"
                    onClicked: {
                         var title = inputField.text.trim();
                         // 章节允许空标题，后端会兜底生成默认标题
                         var allowEmpty = (inputDialog.actionType === "chapter");
                         if (title !== "" || allowEmpty) {
                             if (inputDialog.actionType === "volume") {
                                 projectController.createVolume(inputDialog.projectId, title);
                             } else if (inputDialog.actionType === "chapter") {
                                 projectController.createChapter(inputDialog.projectId, inputDialog.volumeId, title);
                             } else if (inputDialog.actionType === "rename_project") {
                                 projectController.renameProject(inputDialog.projectId, title);
                             } else if (inputDialog.actionType === "rename_volume") {
                                 projectController.renameVolume(inputDialog.projectId, inputDialog.volumeId, title);
                             } else if (inputDialog.actionType === "rename_chapter") {
                                 projectController.renameChapter(inputDialog.projectId, inputDialog.volumeId, inputDialog.chapterId, title);
                             }
                         }
                         inputDialog.close();
                     }
                }
            }
        }
        onOpened: {
            inputField.text = inputDialog.defaultText;
        }
    }
}
