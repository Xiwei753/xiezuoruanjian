//! Issue #948 — Linux Qt 同步结果：分离"本轮运行/终态"与"持久历史"。
//!
//! 2026-10-18 诊断包显示 `full_sync_state.json` 为 `recoverable_error`（10 月 17 日
//! 20:52 的旧失败），`last_sync_report.json=null`，但 10 月 18 日本轮启动后只做了
//! `load_sync_config`，没有触发任何同步。旧实现把 Core 恢复出来的持久历史状态
//! 直接渲染成"同步失败"，既没有尝试时间，也分不清是不是本轮的结果。
//!
//! QML 组件依赖 qrc 与 Rust 注册的上下文属性，无法在本仓库的 Rust 测试里实例化
//! （QML 由本地 qmllint 做语法检查），因此按仓库既有 WHITE_BOX 守卫惯例
//! （issue762 / issue832 系列）读取源码，确定性断言本次修复的接线不变量：
//!
//! 1. `SyncBackend.last_full_sync_state` 是只读属性，getter 经 `with_app` 直读 Core
//!    `load_full_sync_state()`（overall_status / last_attempt_time /
//!    last_success_time / failed_targets），无历史时为空；整轮结果不从
//!    `AppSyncState.status` 猜。
//! 2. `SyncPage` 结果区只有一个优先级：本轮运行中 > 本轮操作（operation_id 过滤）
//!    终态 > Core 持久化历史；历史时间戳按本地时间显示「上次同步（…）」。
//! 3. 没有 FullSyncReport（或报告不属于最后尝试那一轮）时不猜测失败原因：用
//!    full_sync_state 说明历史状态与失败 target，并明确「历史记录没有分组详情」，
//!    不再回退到笼统的「同步失败」。
//! 4. 持久报告与诊断导出同源：导出附件仍直读 Core 的 full_sync_state /
//!    last_report，Qt 侧不另存第二份同步历史。
//!
//! 2026-10-18 复核轮补强（评审结论「暂不可关闭」）：
//! - `updatePersistedSyncReportText` 开头必须有「报告属于最近一次」门禁，
//!   日期对不上/解析异常都退回 `updateHistorySyncResultText` 历史文案；
//! - `opIsCurrent` 只有非空 `activeOperationId` + operation_id 相等 +
//!   operation_kind 相等才算本轮，空 ID 不通配后端残留操作；
//! - 「运行诊断」按钮的 `activeOperationKind` 必须是后端实际返回的
//!   `"diagnose"`（不是试运行的 `"dry_run"`）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

/// 返回 apps/Linux_qt 根目录。
fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

/// 读取指定相对路径源文件的完整内容。
fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 取从 `marker` 开始的最多 `len` 字节窗口，结束位置回退到最近的 UTF-8 边界。
fn window_from(src: &str, marker: &str, len: usize) -> String {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker `{marker}` must exist"));
    let target = (start + len).min(src.len());
    let end = (start..=target)
        .rev()
        .find(|i| src.is_char_boundary(*i))
        .unwrap_or(start);
    src[start..end].to_string()
}

/// 去掉整行 `//` 注释，只留可执行语句。
///
/// 守卫断言的是"代码不再出现某模式"，注释里说明历史原因提到该模式不算违规。
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const BACKEND: &str = "src/backend/sync_backend.rs";
const SETTINGS: &str = "src/backend/settings_backend.rs";
const SYNC_PAGE: &str = "qml/SyncPage.qml";

// ─────────────────────────────────────────────────────────────────────────
// 1. SyncBackend：持久历史是只读事实，与"正在执行"分开
// ─────────────────────────────────────────────────────────────────────────

/// `last_full_sync_state` 必须是只读属性，并以 sync_status_changed 通知刷新。
#[test]
fn backend_exposes_readonly_last_full_sync_state() {
    let src = read_src(BACKEND);
    let prop = window_from(&src, "    last_full_sync_state:", 120);
    assert!(
        prop.contains("qt_property!(QString; READ last_full_sync_state"),
        "last_full_sync_state 必须声明为 QString 只读属性，实际窗口:\n{prop}"
    );
    assert!(
        prop.contains("NOTIFY sync_status_changed"),
        "last_full_sync_state 变化必须复用 sync_status_changed 通知，实际窗口:\n{prop}"
    );
    assert!(
        !prop.contains("WRITE"),
        "持久历史状态只读，QML 不得写回，实际窗口:\n{prop}"
    );

    let getter = window_from(&src, "fn last_full_sync_state(&self)", 400);
    assert!(
        getter.contains("with_app") && getter.contains("last_full_sync_state_json()"),
        "getter 必须经 with_app 直读 Core 侧快照，实际窗口:\n{getter}"
    );
}

/// 快照字段取自 Core 的 FullSyncState，不从 AppSyncState 猜整轮结果。
#[test]
fn full_sync_state_json_comes_from_core_full_sync_state() {
    let src = read_src(BACKEND);
    let getter = window_from(&src, "pub(crate) fn last_full_sync_state_json(&self)", 420);
    assert!(
        getter.contains("api.load_full_sync_state()"),
        "必须读 Core 的 FullSyncState，实际窗口:\n{getter}"
    );
    assert!(
        getter.contains("serde_json::to_string(&state)"),
        "FullSyncStateDto 必须按 Core 字段序列化（overall_status/last_attempt_time/last_success_time/failed_targets），\
         实际窗口:\n{getter}"
    );
    assert!(
        getter.contains("QString::from(\"\")"),
        "Core 没有历史时明确返回空串，实际窗口:\n{getter}"
    );
    assert!(
        !getter.contains("load_app_sync_state"),
        "整轮结果不得从单独 AppSyncState.status 猜测，实际窗口:\n{getter}"
    );
    assert!(
        !getter.contains("save_full_sync_state"),
        "Qt 侧只读快照，不得回写 Core 持久状态，实际窗口:\n{getter}"
    );
}

/// 恢复历史状态仍然是"历史"，代码里必须写明它不代表本轮刚失败。
#[test]
fn restored_status_is_documented_as_history() {
    let src = read_src(BACKEND);
    let restore = window_from(&src, "pub(crate) fn restore_sync_status_from_runtime(&mut self)", 2400);
    assert!(
        restore.contains("api.load_full_sync_state()"),
        "本轮运行互斥与历史恢复逻辑保留，实际窗口:\n{restore}"
    );
    assert!(
        restore.contains("历史结果"),
        "必须写明恢复出来的状态是持久历史结果，实际窗口:\n{restore}"
    );
    assert!(
        restore.contains("last_full_sync_state"),
        "历史事实由只读的 last_full_sync_state 单独展示，实际窗口:\n{restore}"
    );
}

/// 同步结束：通知 QML 刷新同一份 Core 状态，并在报告缺失时记录操作 ID/阶段/目标/错误码。
#[test]
fn sync_completion_notifies_and_logs_report_gap() {
    let src = read_src(BACKEND);
    let handle =
        window_from(&src, "pub(crate) fn handle_outcome(&mut self, outcome: SyncTaskOutcome)", 2400);
    assert!(
        handle.contains("self.sync_status_changed()"),
        "完成时必须通知 sync_status_changed，QML 据此重读 Core 全量状态与最新报告，\
         实际窗口:\n{handle}"
    );
    assert!(
        handle.contains("self.log_full_sync_report_gap("),
        "完成路径必须核对本轮是否产出 FullSyncReport，实际窗口:\n{handle}"
    );

    let log = window_from(&src, "fn log_full_sync_report_gap(&self", 2200);
    assert!(
        log.contains("load_latest_full_sync_report()"),
        "必须直读 Core 最新报告判断缺口，实际窗口:\n{log}"
    );
    assert!(
        log.contains("full_sync_report_missing_after_sync"),
        "报告缺失必须有可检索的日志事件名，实际窗口:\n{log}"
    );
    for field in [
        "operation_id",
        "phase",
        "failed_targets",
        "raw_error",
        "outcome_status",
    ] {
        assert!(
            log.contains(field),
            "报告缺失日志必须包含 {field}，实际窗口:\n{log}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 2. SyncPage：显示优先级 = 运行中 > 本轮终态 > 持久历史
// ─────────────────────────────────────────────────────────────────────────

/// 结果区优先级只有一个入口，顺序固定，且保留 operation_id 过滤。
#[test]
fn sync_result_priority_running_then_current_then_history() {
    let src = read_src(SYNC_PAGE);
    let func = window_from(&src, "function updateSyncResultText()", 2600);

    let running = func
        .find("if (root.currentSyncInProgress)")
        .unwrap_or_else(|| panic!("缺少运行中分支，实际窗口:\n{func}"));
    let current = func
        .find("if (opIsCurrent)")
        .unwrap_or_else(|| panic!("缺少本轮操作终态分支，实际窗口:\n{func}"));
    let history = func
        .find("root.updatePersistedSyncReportText()")
        .unwrap_or_else(|| panic!("缺少持久历史分支，实际窗口:\n{func}"));
    assert!(
        running < current,
        "运行中必须优先于本轮终态，实际窗口:\n{func}"
    );
    assert!(
        current < history,
        "本轮操作终态必须优先于持久历史，实际窗口:\n{func}"
    );

    assert!(
        func.contains("root.activeOperationId !== \"\""),
        "空 activeOperationId 表示本页没有可归属的操作，不得通配后端残留操作，实际窗口:\n{func}"
    );
    assert!(
        func.contains("obj.operation_id === root.activeOperationId"),
        "本轮过滤必须精确比较 operation_id，实际窗口:\n{func}"
    );
    assert!(
        func.contains("obj.operation_kind === root.activeOperationKind"),
        "本轮过滤必须精确比较 operation_kind（诊断/试运行不得冒充本轮同步），实际窗口:\n{func}"
    );
    assert!(
        func.contains("root.applyOperationStateText(obj)"),
        "运行中阶段与终态都必须展示实际操作状态，实际窗口:\n{func}"
    );

    // 优先级 3 是最后一个分支：只读 Core 持久历史，不再提前 return 覆盖。
    let tail = &func[func
        .find("// 优先级 3")
        .unwrap_or_else(|| panic!("必须显式标注优先级 3，实际窗口:\n{func}"))..];
    assert!(
        tail.contains("root.updatePersistedSyncReportText()"),
        "无本轮操作时必须落到持久历史，实际窗口:\n{tail}"
    );
}

/// 单击"执行同步"后新操作 ID/结果取代历史展示。
#[test]
fn new_operation_result_replaces_history() {
    let src = read_src(SYNC_PAGE);
    let click = window_from(&src, "text: qsTr(\"执行同步\")", 1800);
    assert!(
        click.contains("root.activeOperationId = opId"),
        "点击执行同步必须记录新操作 ID，实际窗口:\n{click}"
    );
    assert!(
        click.contains("root.activeOperationKind = \"sync\""),
        "点击执行同步必须标记操作类型，实际窗口:\n{click}"
    );

    let func = window_from(&src, "function updateSyncResultText()", 2600);
    assert!(
        !func.replace('\n', " ").contains("\"同步状态解析失败\""),
        "本轮操作状态解析失败不应当作结果文字覆盖，实际窗口:\n{func}"
    );
}

/// 「运行诊断」按钮的 activeOperationKind 必须与后端实际返回的 operation_kind 一致。
#[test]
fn diagnose_button_kind_matches_backend_diagnose_literal() {
    let src = read_src(SYNC_PAGE);
    let click = window_from(&src, "text: qsTr(\"运行诊断\")", 1600);
    assert!(
        click.contains("perform_sync_diagnostics()"),
        "运行诊断按钮必须调用后端诊断入口，实际窗口:\n{click}"
    );
    assert!(
        click.contains("root.activeOperationKind = \"diagnose\""),
        "运行诊断的 activeOperationKind 必须写后端实际返回的 \"diagnose\"，实际窗口:\n{click}"
    );
    let stripped = strip_line_comments(&src);
    assert!(
        !stripped.contains("activeOperationKind = \"dry_run\""),
        "诊断与试运行是两种操作，QML 不得把诊断标成 \"dry_run\""
    );

    let backend = read_src(BACKEND);
    let diagnose = window_from(&backend, "pub(crate) fn perform_sync_diagnostics(", 3000);
    assert!(
        diagnose.contains("operation_kind: \"diagnose\""),
        "后端诊断操作的 operation_kind 事实来源是 \"diagnose\"，实际窗口:\n{diagnose}"
    );
}

/// 本轮同步只有确实产出了属于这一轮的报告才用报告详情覆盖终态。
#[test]
fn persisted_report_overrides_only_when_it_belongs_to_latest_attempt() {
    let src = read_src(SYNC_PAGE);
    let func = window_from(&src, "function updateSyncResultText()", 2600);
    assert!(
        func.contains("obj.operation_kind === \"sync\"")
            && func.contains("root.syncReportBelongsToLatestAttempt()"),
        "本轮同步用报告详情覆盖终态前，必须确认报告属于最后这一轮，实际窗口:\n{func}"
    );

    let guard = window_from(&src, "function syncReportBelongsToLatestAttempt()", 1900);
    assert!(
        guard.contains("report.attempted_at") && guard.contains("state.last_attempt_time"),
        "必须比较报告 attempted_at 与 Core 记录的 last_attempt_time，实际窗口:\n{guard}"
    );
    assert!(
        guard.contains("report.attempted_at <= 0") && guard.contains("state.last_attempt_time <= 0"),
        "attempted_at / last_attempt_time 缺失或为 0（无历史）必须按不属于本轮处理，\
         实际窗口:\n{guard}"
    );
}

/// 持久报告展示前必须有「报告属于最近一次」门禁，失败/解析异常退回历史文案。
#[test]
fn persisted_report_requires_latest_attempt_gate() {
    let src = read_src(SYNC_PAGE);
    let func = window_from(&src, "function updatePersistedSyncReportText()", 800);
    let gate = func
        .find("root.syncReportBelongsToLatestAttempt()")
        .unwrap_or_else(|| panic!("开头必须有属于最近一次的门禁，实际窗口:\n{func}"));
    let body = func
        .find("JSON.parse(root.lastSyncReport)")
        .unwrap_or_else(|| panic!("门禁之后解析报告 JSON，实际窗口:\n{func}"));
    assert!(
        gate < body,
        "门禁必须在解析/展示报告详情之前，实际窗口:\n{func}"
    );
    let gate_window = &func[gate..body];
    assert!(
        gate_window.contains("root.updateHistorySyncResultText()"),
        "门禁不通过（日期对不上/缺 lastFullSyncState）必须退回历史文案，实际窗口:\n{gate_window}"
    );

    let parse = window_from(&func, "JSON.parse(root.lastSyncReport)", 260);
    assert!(
        parse.contains("root.updateHistorySyncResultText()"),
        "报告解析异常必须退回历史文案，不得继续展示，实际窗口:\n{parse}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. 历史展示：本地时间 + 失败 target + 无报告说明
// ─────────────────────────────────────────────────────────────────────────

/// 时间戳是 Unix 秒数，必须以本地时间格式化。
#[test]
fn history_timestamps_are_rendered_in_local_time() {
    let src = read_src(SYNC_PAGE);
    let func = window_from(&src, "function formatSyncLocalTime(seconds)", 320);
    assert!(
        func.contains("new Date(seconds * 1000)"),
        "Unix 秒必须乘 1000 交给 Date，实际窗口:\n{func}"
    );
    assert!(
        func.contains("toLocaleString()"),
        "必须按本地时间显示，实际窗口:\n{func}"
    );
}

/// 没有新一轮同步时，读 Core 持久化的 full_sync_state 表述"上次同步"。
#[test]
fn history_text_uses_core_full_sync_state_facts() {
    let src = read_src(SYNC_PAGE);
    assert!(
        src.contains("root.lastFullSyncState = root.syncBackendRef.last_full_sync_state"),
        "refreshLocalSyncState 必须读取只读的 last_full_sync_state"
    );
    assert!(
        !src.contains("last_full_sync_state ="),
        "QML 不得写回历史状态，只读展示"
    );

    let func = window_from(&src, "function updateHistorySyncResultText()", 1600);
    for marker in [
        "root.lastFullSyncState",
        "state.last_attempt_time",
        "state.last_success_time",
        "state.failed_targets",
        "state.overall_status",
    ] {
        assert!(
            func.contains(marker),
            "历史展示必须使用 Core FullSyncState 字段 {marker}，实际窗口:\n{func}"
        );
    }
    assert!(
        func.contains("上次同步"),
        "历史标题必须写明这是上次同步，实际窗口:\n{func}"
    );
    assert!(
        func.contains("formatSyncLocalTime(state.last_attempt_time)"),
        "历史标题必须带最后尝试时间，实际窗口:\n{func}"
    );
    assert!(
        func.contains("syncStatusLabel(state.overall_status)"),
        "历史状态必须复用同一份状态文案映射，实际窗口:\n{func}"
    );
    assert!(
        func.contains("失败目标"),
        "有失败 target 时必须单独列出，实际窗口:\n{func}"
    );
}

/// 报告为空时用历史状态说明，并解释"历史记录没有分组详情"。
#[test]
fn missing_report_explains_absent_group_detail() {
    let src = read_src(SYNC_PAGE);
    let func = window_from(&src, "function updatePersistedSyncReportText()", 2800);
    let no_report = func
        .find("if (!report)")
        .unwrap_or_else(|| panic!("必须有报告为空的处理分支，实际窗口:\n{func}"));
    let branch_end = (no_report + 200).min(func.len());
    let branch = &func[no_report..branch_end];
    assert!(
        branch.contains("root.updateHistorySyncResultText()"),
        "报告为空必须转到 full_sync_state 历史展示，实际窗口:\n{branch}"
    );

    let history = window_from(&src, "function updateHistorySyncResultText()", 1600);
    assert!(
        history.contains("历史记录没有分组详情"),
        "旧记录没有分组明细时必须明确说明，实际窗口:\n{history}"
    );
    assert!(
        history.contains("root.isFailureStatus(state.overall_status)")
            && history.contains("failedTargets.length > 0"),
        "说明只在失败状态或存在失败 target 时附加，实际窗口:\n{history}"
    );
}

/// 恢复出来的历史状态不得再被渲染成笼统的「同步失败」四字。
#[test]
fn refresh_local_state_does_not_render_history_as_fresh_failure() {
    let src = read_src(SYNC_PAGE);
    let func = strip_line_comments(&window_from(&src, "function refreshLocalSyncState()", 1400));
    assert!(
        !func.contains("qsTr(\"同步失败\")"),
        "不得用 isFailureStatus(currentSyncStatus) 直接把历史状态写成「同步失败」，\
         实际代码:\n{func}"
    );
    assert!(
        func.contains("root.updateSyncResultText()"),
        "结果区必须走统一优先级入口，实际代码:\n{func}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. 持久报告仍完整展示（#931/#939 不回退）+ 诊断导出同源
// ─────────────────────────────────────────────────────────────────────────

/// 有报告时继续显示分组详情，不被"分离历史"改掉。
#[test]
fn persisted_report_detail_is_preserved() {
    let src = read_src(SYNC_PAGE);
    let func = window_from(&src, "function updatePersistedSyncReportText()", 2800);
    for marker in [
        "report.completed_groups",
        "report.failed_groups",
        "failure.group_id",
        "failure.path",
        "failure.phase",
        "failure.error_category",
        "failure.detail",
    ] {
        assert!(
            func.contains(marker),
            "持久报告必须继续展示 {marker}，实际窗口:\n{func}"
        );
    }
}

/// 诊断包与页面展示共用 Core 的同一份持久状态，Qt 侧不另存历史。
#[test]
fn diagnostics_export_shares_the_same_core_state_source() {
    let export = read_src(SETTINGS);
    let attachments = window_from(&export, "fn build_export_attachments(", 8000);
    assert!(
        attachments.contains("api.load_full_sync_state()"),
        "诊断导出的 full_sync_state.json 必须直读 Core，实际窗口:\n{attachments}"
    );
    assert!(
        attachments.contains("\"full_sync_state.json\""),
        "诊断包必须包含 full_sync_state.json，实际窗口:\n{attachments}"
    );
    assert!(
        attachments.contains("api.load_latest_full_sync_report()"),
        "诊断导出的 last_sync_report.json 必须直读 Core，实际窗口:\n{attachments}"
    );
    assert!(
        attachments.contains("\"last_sync_report.json\""),
        "诊断包必须包含 last_sync_report.json，实际窗口:\n{attachments}"
    );

    let qml = read_src(SYNC_PAGE);
    assert!(
        !qml.contains("full_sync_state.json") && !qml.contains("last_sync_report.json"),
        "QML 不得读写同步历史文件，历史只能来自 SyncBackend 的 Core 快照"
    );
}
