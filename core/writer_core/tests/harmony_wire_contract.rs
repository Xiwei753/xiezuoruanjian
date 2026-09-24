//! Harmony 线路契约基线（Issue #753 评论 5809590165）。
//!
//! 逐条调用 Harmony NAPI 实际用到的 `writer_core_*` 入口，把 envelope 里
//! `data` 的真实 JSON 形状（字段名、嵌套层级、类型）固化到
//! `tests/fixtures/harmony_wire_contract.json`。
//!
//! ArkTS `corebridge/codec/CoreWireDecoders.ets` 的 decoder 必须与该基线一致：
//! `tools/check_harmony_dto_contract.py` 用同一份基线校验 ArkTS 端字段名。
//! Core 改动序列化形状时本测试先失败，避免漂移再次流向 UI。
//!
//! 重新生成基线：
//! `WRITE_HARMONY_WIRE_CONTRACT=1 cargo test -p writer_core --features harmony-ffi --test harmony_wire_contract`

#![cfg(feature = "harmony-ffi")]
#![allow(clippy::expect_used, clippy::undocumented_unsafe_blocks)]

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use writer_core::ffi;

/// 一个 JSON 节点的形状。数组只记录元素形状（多元素取并集）。
/// 标量类型带 `?` 后缀表示该字段可空/只在部分元素出现。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Node {
    Object { fields: BTreeMap<String, Node> },
    Array { element: Option<Box<Node>> },
    Scalar { ty: String },
    Optional { inner: Box<Node> },
    Mixed { variants: Vec<Node> },
}

fn shape_of(value: &Value) -> Node {
    match value {
        Value::Object(map) => {
            let mut fields = BTreeMap::new();
            for (k, v) in map {
                fields.insert(k.clone(), shape_of(v));
            }
            Node::Object { fields }
        }
        Value::Array(items) => {
            let mut merged: Option<Node> = None;
            for item in items {
                let next = shape_of(item);
                merged = Some(match merged {
                    None => next,
                    Some(prev) => merge(prev, next),
                });
            }
            Node::Array {
                element: merged.map(Box::new),
            }
        }
        Value::Null => Node::Scalar {
            ty: "null".to_string(),
        },
        Value::Bool(_) => Node::Scalar {
            ty: "boolean".to_string(),
        },
        Value::Number(_) => Node::Scalar {
            ty: "number".to_string(),
        },
        Value::String(_) => Node::Scalar {
            ty: "string".to_string(),
        },
    }
}

fn merge(a: Node, b: Node) -> Node {
    match (a, b) {
        (Node::Object { fields: f1 }, Node::Object { fields: f2 }) => {
            let mut merged_fields: BTreeMap<String, Node> = BTreeMap::new();
            for (k, v1) in f1 {
                let node = match f2.get(&k) {
                    // 两侧都有：按值合并，不降级成可选。
                    Some(v2) => merge(v1, v2.clone()),
                    // 这一侧缺席：该字段不是每条元素都有。
                    None => optional(v1),
                };
                merged_fields.insert(k, node);
            }
            for (k, v2) in f2 {
                merged_fields.entry(k).or_insert_with(|| optional(v2));
            }
            Node::Object {
                fields: merged_fields,
            }
        }
        (Node::Array { element: e1 }, Node::Array { element: e2 }) => Node::Array {
            element: match (e1, e2) {
                (Some(x), Some(y)) => Some(Box::new(merge(*x, *y))),
                (Some(x), None) => Some(x),
                (None, y) => y,
            },
        },
        (Node::Scalar { ty: t1 }, Node::Scalar { ty: t2 }) => {
            // 只在"是否一定出现"上有差别的同名标量合并成可选形式，
            // 否则三元素数组会叠出 mixed(number?, number?, number?)。
            let base1 = t1.trim_end_matches('?');
            let base2 = t2.trim_end_matches('?');
            if base1 == base2 {
                let optional_ty = t1.ends_with('?') || t2.ends_with('?');
                Node::Scalar {
                    ty: if optional_ty {
                        format!("{base1}?")
                    } else {
                        base1.to_string()
                    },
                }
            } else {
                let mut variants = Vec::new();
                push_variants(&mut variants, Node::Scalar { ty: t1 });
                push_variants(&mut variants, Node::Scalar { ty: t2 });
                Node::Mixed { variants }
            }
        }
        (a, b) => {
            let mut variants = Vec::new();
            push_variants(&mut variants, a);
            push_variants(&mut variants, b);
            Node::Mixed { variants }
        }
    }
}

fn push_variants(out: &mut Vec<Node>, node: Node) {
    match node {
        Node::Mixed { variants } => {
            for v in variants {
                push_variants(out, v);
            }
        }
        other => out.push(other),
    }
}

fn optional(node: Node) -> Node {
    match node {
        Node::Scalar { mut ty } => {
            if ty != "null" && !ty.ends_with('?') {
                ty.push('?');
            }
            Node::Scalar { ty }
        }
        Node::Mixed { variants } => Node::Mixed {
            variants: variants.into_iter().map(optional).collect(),
        },
        other => Node::Optional {
            inner: Box::new(other),
        },
    }
}

type Baseline = BTreeMap<String, Node>;

fn render(node: &Node) -> String {
    serde_json::to_string(node).unwrap_or_default()
}

fn take_string(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: ptr 由本测试刚从 writer_core_* 取回，非空且指向 NUL 结尾 UTF-8。
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
    // SAFETY: ptr 来自 writer_core_* 返回值，按 FFI 契约由调用方释放一次。
    unsafe { ffi::writer_core_free_string(ptr) };
    s
}

fn cstr(s: &str) -> CString {
    CString::new(s.to_string()).expect("测试入参不含内嵌 NUL")
}

/// 记录一次调用的 `data` 形状，并要求该调用成功（失败说明契约本身不成立）。
fn note(out: &mut Baseline, endpoint: &str, json: &str) -> Value {
    let parsed: Value = serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("{endpoint} 返回非法 JSON: {e}; raw={json}"));
    let ok = parsed
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        ok,
        "{endpoint} 期望成功，实际返回: {json}\n\
         （若是 Core 契约本身变了，重新生成基线并同步 ArkTS decoder）"
    );
    out.insert(endpoint.to_string(), shape_of(&parsed["data"]));
    if std::env::var("HARMONY_WIRE_DEBUG").is_ok() {
        eprintln!("[{endpoint}] {}", parsed["data"]);
    }
    parsed
}

/// 只记录调用是否成功，不固化形状（用于入参形态由平台决定的端点）。
fn note_ok(out: &mut Baseline, endpoint: &str, json: &str) -> Value {
    let parsed: Value = serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("{endpoint} 返回非法 JSON: {e}; raw={json}"));
    if parsed.get("success").and_then(Value::as_bool) == Some(true) {
        out.insert(endpoint.to_string(), shape_of(&parsed["data"]));
        if std::env::var("HARMONY_WIRE_DEBUG").is_ok() {
            eprintln!("[{endpoint}] {}", parsed["data"]);
        }
    } else if std::env::var("HARMONY_WIRE_DEBUG").is_ok() {
        eprintln!("[{endpoint}] SKIPPED {json}");
    }
    parsed
}

fn init_core() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cstr(&dir.path().to_string_lossy());
    // SAFETY: path 是本测试持有的合法 NUL 结尾 UTF-8 路径。
    let code = unsafe { ffi::writer_core_init(path.as_ptr()) };
    assert_eq!(code, 0, "writer_core_init 失败: {code}");
    dir
}

fn call0(f: unsafe extern "C" fn() -> *mut c_char) -> String {
    // SAFETY: 无入参的只读 FFI 入口；返回值立即复制并释放。
    take_string(unsafe { f() })
}

fn call1(f: unsafe extern "C" fn(*const c_char) -> *mut c_char, arg: &CString) -> String {
    // SAFETY: arg 是本测试持有的合法 C 串。
    take_string(unsafe { f(arg.as_ptr()) })
}

fn call2(
    f: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_char,
    a: &CString,
    b: &CString,
) -> String {
    // SAFETY: a/b 都是本测试持有的合法 C 串。
    take_string(unsafe { f(a.as_ptr(), b.as_ptr()) })
}

fn call3(
    f: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> *mut c_char,
    a: &CString,
    b: &CString,
    c: &CString,
) -> String {
    // SAFETY: 三个指针都是本测试持有的合法 C 串。
    take_string(unsafe { f(a.as_ptr(), b.as_ptr(), c.as_ptr()) })
}

fn str_at(env: &Value, pointer: &str) -> String {
    let mut cur = &env["data"];
    for seg in pointer.split('.') {
        cur = &cur[seg];
    }
    match cur {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn num_at(env: &Value, pointer: &str) -> u64 {
    let mut cur = &env["data"];
    for seg in pointer.split('.') {
        cur = &cur[seg];
    }
    cur.as_u64()
        .unwrap_or_else(|| panic!("{pointer} 不是无符号整数: {cur}"))
}

/// 部分 C ABI 直接返回标量（会话 id、revision、grapheme 边界），没有对象壳。
fn scalar_u64(env: &Value, endpoint: &str) -> u64 {
    env["data"]
        .as_u64()
        .unwrap_or_else(|| panic!("{endpoint} 不是无符号整数标量: {}", env["data"]))
}

/// composition 身份字段同时出现在 `compositionSession` 与 `composition` 两处，
/// 按 Core 实际发出的那个键取值。
fn first_num(env: &Value, pointers: &[&str]) -> u64 {
    for pointer in pointers {
        let mut cur = &env["data"];
        for seg in pointer.split('.') {
            cur = &cur[seg];
        }
        if let Some(n) = cur.as_u64() {
            return n;
        }
    }
    panic!("{pointers:?} 中没有无符号整数字段: {}", env["data"]);
}

fn ids_of(env: &Value) -> Vec<String> {
    env["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 数值按 f64 比较（Core 侧 f64 字段会序列化成 `260.0`），其余按值比较。
fn assert_field(data: &Value, field: &str, expected: &str) {
    let got = data.get(field).cloned().unwrap_or(Value::Null);
    let want: Value = serde_json::from_str(expected).expect("测试期望值合法");
    let same = match (&got, &want) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (x, y) => x == y,
    };
    assert!(
        same,
        "save→load 后 {field} 不一致: got={got} want={want}\n完整 data = {data}"
    );
}

/// 手造一份含 2 节点 1 边的 `StarMapGraphDto` JSON，用来观察边渲染输出形状。
fn seeded_graph_json(starmap_id: &str) -> String {
    format!(
        r#"{{
      "schemaVersion": 1,
      "id": "{starmap_id}",
      "starmapId": "{starmap_id}",
      "title": "契约基线",
      "nodes": [
        {{"id":"n1","title":"节点一","kind":"Concept","payload":null,"createdAt":0,"updatedAt":0}},
        {{"id":"n2","title":"节点二","kind":"Concept","payload":null,"createdAt":0,"updatedAt":0}}
      ],
      "edges": [
        {{"id":"e1","from":"n1","to":"n2","kind":"RelatedTo","label":null,"payload":null,"createdAt":0,"updatedAt":0}}
      ],
      "embeds": [],
      "links": [],
      "hyperlinks": [],
      "createdAt": 0,
      "updatedAt": 0
    }}"#
    )
}

#[test]
#[allow(clippy::too_many_lines)]
fn harmony_wire_contract_matches_fixture() {
    let _dir = init_core();
    let mut out: Baseline = Baseline::new();

    // ── settings ──
    use ffi::settings_ops as settings;
    let _ = note(
        &mut out,
        "loadLocalSettings",
        &call0(settings::writer_core_load_local_settings),
    );
    let patch = cstr(
        r#"{"editorFontSize":17.5,"editorLineSpacingMultiplier":1.75,"autoSaveEnabled":false,
            "autoSaveDelayMs":1500,"autoIndentEnabled":true,"autoIndentWidth":2,
            "desktopSidebarWidth":260,"desktopEditorWidth":720,
            "editorCoordinatedTextCursorAnimationEnabled":true,
            "diagnosticsEnabled":true,"diagnosticsVerbose":false,"appearanceMode":"dark"}"#,
    );
    let _ = note(
        &mut out,
        "saveLocalSettings",
        &call1(settings::writer_core_save_local_settings, &patch),
    );
    let loaded = note(
        &mut out,
        "loadLocalSettingsAfterPatch",
        &call0(settings::writer_core_load_local_settings),
    );
    // 评论 1/2：写入的 camelCase 字段必须原样读回，Core 只有一张字段表。
    let data = &loaded["data"];
    for (field, expected) in [
        ("editorFontSize", "17.5"),
        ("editorLineSpacingMultiplier", "1.75"),
        ("autoSaveEnabled", "false"),
        ("autoSaveDelayMs", "1500"),
        ("autoIndentEnabled", "true"),
        ("autoIndentWidth", "2"),
        ("desktopSidebarWidth", "260"),
        ("desktopEditorWidth", "720"),
        ("editorCoordinatedTextCursorAnimationEnabled", "true"),
        ("diagnosticsEnabled", "true"),
        ("diagnosticsVerbose", "false"),
        ("appearanceMode", "\"dark\""),
    ] {
        assert_field(data, field, expected);
    }

    let _ = note(
        &mut out,
        "loadSyncableSettings",
        &call0(settings::writer_core_load_syncable_settings),
    );
    let syncable_patch =
        cstr(r#"{"fontSize":18,"themeMode":"dark","monetColor":"custom","themePaletteJson":"{}"}"#);
    let _ = note(
        &mut out,
        "saveSyncableSettings",
        &call1(
            settings::writer_core_save_syncable_settings,
            &syncable_patch,
        ),
    );
    let _ = note(
        &mut out,
        "listPaletteRecords",
        &call0(settings::writer_core_list_palette_records),
    );
    let _ = note(
        &mut out,
        "listBuiltinThemes",
        &call0(settings::writer_core_list_builtin_themes),
    );

    // ── project / volume / chapter ──
    use ffi::project_ops as project;
    let name = cstr("测试作品");
    let created_project = note(
        &mut out,
        "createProject",
        &call1(project::writer_core_create_project, &name),
    );
    let project_id = str_at(&created_project, "id");
    let project_c = cstr(&project_id);

    let volume_c = cstr("第一卷");
    let created_volume = note(
        &mut out,
        "createVolume",
        &call2(project::writer_core_create_volume, &project_c, &volume_c),
    );
    let volume_id = str_at(&created_volume, "id");
    let volume_c = cstr(&volume_id);

    let chapter_c = cstr("第一章");
    let created_chapter = note(
        &mut out,
        "createChapter",
        &call3(
            project::writer_core_create_chapter,
            &project_c,
            &volume_c,
            &chapter_c,
        ),
    );
    let chapter_id = str_at(&created_chapter, "id");
    let chapter_c = cstr(&chapter_id);

    let _ = note(
        &mut out,
        "listProjects",
        &call0(project::writer_core_list_projects),
    );
    let _ = note(
        &mut out,
        "getProjectTree",
        &call1(project::writer_core_get_project_tree, &project_c),
    );
    let volumes_env = note(
        &mut out,
        "listVolumes",
        &call1(project::writer_core_list_volumes, &project_c),
    );
    let chapters_env = note(
        &mut out,
        "listChapters",
        &call2(project::writer_core_list_chapters, &project_c, &volume_c),
    );
    let _ = note(
        &mut out,
        "getProjectStats",
        &call1(project::writer_core_get_project_stats, &project_c),
    );
    let body = cstr("你好 world");
    let _ = note(
        &mut out,
        "saveChapter",
        // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            project::writer_core_save_chapter(
                project_c.as_ptr(),
                volume_c.as_ptr(),
                chapter_c.as_ptr(),
                body.as_ptr(),
            )
        }),
    );
    let opened = note(
        &mut out,
        "openChapter",
        &call3(
            project::writer_core_open_chapter,
            &project_c,
            &volume_c,
            &chapter_c,
        ),
    );
    // 评论 4：open_chapter 必须是 { meta, content }，不再是扁平结构。
    assert!(
        opened["data"].get("meta").is_some(),
        "openChapter 缺少 meta 包裹"
    );
    assert_eq!(str_at(&opened, "content"), "你好 world");
    let _ = note(
        &mut out,
        "renameProject",
        &call2(
            project::writer_core_rename_project,
            &project_c,
            &cstr("改名作品"),
        ),
    );
    let _ = note(
        &mut out,
        "renameVolume",
        &call3(
            project::writer_core_rename_volume,
            &project_c,
            &volume_c,
            &cstr("改名卷"),
        ),
    );
    let _ = note(
        &mut out,
        "renameChapter",
        // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            project::writer_core_rename_chapter(
                project_c.as_ptr(),
                volume_c.as_ptr(),
                chapter_c.as_ptr(),
                cstr("改名章").as_ptr(),
            )
        }),
    );
    let all_volume_ids =
        cstr(&serde_json::to_string(&ids_of(&volumes_env)).expect("序列化卷 id 列表"));
    let _ = note(
        &mut out,
        "reorderVolumes",
        &call2(
            project::writer_core_reorder_volumes,
            &project_c,
            &all_volume_ids,
        ),
    );
    let all_chapter_ids =
        cstr(&serde_json::to_string(&ids_of(&chapters_env)).expect("序列化章节 id 列表"));
    let _ = note(
        &mut out,
        "reorderChapters",
        &call3(
            project::writer_core_reorder_chapters,
            &project_c,
            &volume_c,
            &all_chapter_ids,
        ),
    );
    let _ = note(
        &mut out,
        "resolveChapterLocation",
        &call1(
            ffi::app_state_ops::writer_core_resolve_chapter_location,
            &chapter_c,
        ),
    );
    let _ = note(
        &mut out,
        "resolveVolumeLocation",
        &call1(
            ffi::app_state_ops::writer_core_resolve_volume_location,
            &volume_c,
        ),
    );
    let _ = note(
        &mut out,
        "clearChapter",
        &call3(
            project::writer_core_clear_chapter,
            &project_c,
            &volume_c,
            &chapter_c,
        ),
    );

    // ── app state ──
    use ffi::app_state_ops as app_state;
    let _ = note(
        &mut out,
        "getAppState",
        &call0(app_state::writer_core_get_app_state),
    );
    let _ = note(
        &mut out,
        "getRecentEdits",
        &call0(app_state::writer_core_get_recent_edits),
    );

    // ── writing stats ──
    use ffi::writing_stats_ops as stats;
    let event = cstr(&format!(
        r#"{{"deviceId":"d1","platform":"harmony","projectId":"{project_id}","volumeId":"{volume_id}","chapterId":"{chapter_id}","oldText":"","newText":"你好 world","durationSeconds":12,"sessionId":"s1"}}"#
    ));
    let _ = note(
        &mut out,
        "processWritingEvent",
        &call1(stats::writer_core_process_writing_event, &event),
    );
    let _ = note(
        &mut out,
        "getWritingStats",
        &call0(stats::writer_core_get_writing_stats),
    );

    // ── layout / screen policy ──
    use ffi::layout_ops as layout;
    use ffi::screen_policy_ops as screen_policy;
    let viewport = cstr(
        r#"{"widthDp":1280.0,"heightDp":800.0,"occlusions":[{"leftDp":0.0,"topDp":0.0,"rightDp":0.0,"bottomDp":0.0,"separating":false}]}"#,
    );
    let _ = note(
        &mut out,
        "resolveLayout",
        &call1(layout::writer_core_resolve_layout, &viewport),
    );
    for role in ["Home", "Writing", "ProjectWorkspace", "Settings"] {
        let role_json = format!("\"{role}\"");
        let role_c = cstr(&role_json);
        let _ = note(
            &mut out,
            &format!("resolveScreenPolicy.{role}"),
            &call1(screen_policy::writer_core_resolve_screen_policy, &role_c),
        );
    }

    // ── starmap ──
    use ffi::starmap_ops as starmap;
    let created_starmap = note(
        &mut out,
        "createStarMap",
        &call2(
            starmap::writer_core_create_starmap,
            &cstr("人物图"),
            &cstr("描述"),
        ),
    );
    let starmap_id = str_at(&created_starmap, "starmapId");
    let starmap_c = cstr(&starmap_id);
    let _ = note(
        &mut out,
        "listStarMaps",
        &call0(starmap::writer_core_list_starmaps),
    );
    let _ = note(
        &mut out,
        "listStarMapsForProject",
        &call1(starmap::writer_core_list_starmaps_for_project, &project_c),
    );
    let _ = note(
        &mut out,
        "getStarMap",
        &call1(starmap::writer_core_get_starmap, &starmap_c),
    );
    let _ = note(
        &mut out,
        "getStarMapGraph",
        &call1(starmap::writer_core_get_starmap_graph, &starmap_c),
    );
    let _ = note(
        &mut out,
        "getStarMapLayout",
        &call1(starmap::writer_core_get_starmap_layout, &starmap_c),
    );
    let _ = note(
        &mut out,
        "getStarMapMotionPolicy",
        &call0(starmap::writer_core_get_starmap_motion_policy),
    );
    let _ = note(
        &mut out,
        "renameStarMap",
        &call2(
            starmap::writer_core_rename_starmap,
            &starmap_c,
            &cstr("人物图2"),
        ),
    );
    // 两个节点 + 一条边：让边渲染输出带上真实元素形状，同时校验 StarMapGraphDto 入参契约。
    let seeded_layout = cstr(
        r#"{"kind":"Freeform","nodes":[{"nodeId":"n1","x":10.0,"y":10.0,"width":48.0,"height":48.0,"radius":24.0,"collapsed":false,"zIndex":0,"scale":1.0,"depth":0.0,"focusWeight":1.0,"orbitGroup":null},{"nodeId":"n2","x":210.0,"y":150.0,"width":48.0,"height":48.0,"radius":24.0,"collapsed":false,"zIndex":1,"scale":1.0,"depth":0.0,"focusWeight":1.0,"orbitGroup":null}]}"#,
    );
    let _ = note(
        &mut out,
        "saveStarMapLayout",
        &call2(
            starmap::writer_core_save_starmap_layout,
            &starmap_c,
            &seeded_layout,
        ),
    );
    let layout_after_save = note(
        &mut out,
        "getStarMapLayoutAfterSeed",
        &call1(starmap::writer_core_get_starmap_layout, &starmap_c),
    );
    assert_eq!(
        layout_after_save["data"]["nodes"]
            .as_array()
            .map_or(0, |v| v.len()),
        2
    );
    let seeded_graph = cstr(&seeded_graph_json(&starmap_id));
    let renders = note(
        &mut out,
        "computeStarMapEdgeRenders",
        &call1(
            starmap::writer_core_compute_starmap_edge_renders,
            &seeded_graph,
        ),
    );
    assert_eq!(
        renders["data"].as_array().map_or(0, |v| v.len()),
        1,
        "一条边应得到一条渲染数据"
    );
    let _ = note(
        &mut out,
        "saveStarMapViewport",
        &call2(
            starmap::writer_core_save_starmap_viewport,
            &starmap_c,
            &cstr(r#"{"scale":1.25,"offsetX":-30.0,"offsetY":12.0,"width":384.0,"height":640.0}"#),
        ),
    );

    // ── sync ──
    use ffi::sync_ops as sync;
    let _ = note(
        &mut out,
        "loadSyncConfig",
        &call0(sync::writer_core_load_sync_config),
    );
    let _ = note(
        &mut out,
        "saveSyncConfig",
        &call1(
            sync::writer_core_save_sync_config,
            &cstr(
                r#"{"enabled":true,"autoSync":false,"syncIntervalSeconds":600,"activeProvider":"github"}"#,
            ),
        ),
    );
    // patch 之后再整份回写，字段必须原样读回。
    let sync_config = note(
        &mut out,
        "loadSyncConfigAfterPatch",
        &call0(sync::writer_core_load_sync_config),
    );
    assert_eq!(sync_config["data"]["enabled"], Value::Bool(true));
    assert_eq!(sync_config["data"]["syncIntervalSeconds"], Value::from(600));
    assert_eq!(sync_config["data"]["activeProvider"], Value::from("github"));
    let app_sync_state = note(
        &mut out,
        "loadAppSyncState",
        &call0(sync::writer_core_load_app_sync_state),
    );
    // 平台端回写的是 load 得到的同一个 DTO —— 必须能原样往返。
    let _ = note(
        &mut out,
        "saveAppSyncState",
        &call1(
            sync::writer_core_save_app_sync_state,
            &cstr(&app_sync_state["data"].to_string()),
        ),
    );
    // 同步需要平台网络能力，纯 Rust 测试环境里可能直接失败：形状能记就记，
    // 未落地部分由 tools/check_harmony_dto_contract.py 按 Core DTO 静态校验。
    let _ = note_ok(
        &mut out,
        "fullSyncDryRun",
        &call0(sync::writer_core_full_sync_dry_run),
    );
    let _ = note_ok(
        &mut out,
        "fullSyncDiagnostics",
        &call0(sync::writer_core_full_sync_diagnostics),
    );
    let _ = note(
        &mut out,
        "loadDeviceInfo",
        &call0(sync::writer_core_load_device_info),
    );
    let _ = note(
        &mut out,
        "ensureDeviceInfo",
        &call2(
            sync::writer_core_ensure_device_info,
            &cstr("harmony"),
            &cstr("phone"),
        ),
    );

    // ── editor session ──
    use ffi::editor_session_ops as session;
    let target = cstr(&format!("{project_id}/{volume_id}/{chapter_id}"));
    let created_session = note(
        &mut out,
        "editorSessionCreate",
        // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_create(target.as_ptr(), cstr("abc").as_ptr(), 3)
        }),
    );
    let sid = scalar_u64(&created_session, "editorSessionCreate");
    let snapshot = note(
        &mut out,
        "editorSessionSnapshot",
        // SAFETY: sid 是刚创建的合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe { session::writer_core_editor_session_snapshot(sid) }),
    );
    // 期望 revision 必须逐条接力：传 u64::MAX 会被 Core 判成 staleRevision，
    // 基线就会固化成 stale fallback 而不是真实的编辑结果形状。
    let mut revision = num_at(&snapshot, "revision");
    revision = num_at(
        &note(
            &mut out,
            "editorSessionInsert",
            // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
            &take_string(unsafe {
                session::writer_core_editor_session_insert(
                    sid,
                    3,
                    cstr("x").as_ptr(),
                    cstr("Typing").as_ptr(),
                    revision,
                )
            }),
        ),
        "newRevision",
    );
    revision = num_at(
        &note(
            &mut out,
            "editorSessionDelete",
            // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
            &take_string(unsafe {
                session::writer_core_editor_session_delete(
                    sid,
                    3,
                    4,
                    cstr("Delete").as_ptr(),
                    revision,
                )
            }),
        ),
        "newRevision",
    );
    revision = num_at(
        &note(
            &mut out,
            "editorSessionReplace",
            // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
            &take_string(unsafe {
                session::writer_core_editor_session_replace(
                    sid,
                    0,
                    3,
                    cstr("yz").as_ptr(),
                    cstr("abc").as_ptr(),
                    cstr("Paste").as_ptr(),
                    revision,
                )
            }),
        ),
        "newRevision",
    );
    revision = num_at(
        &note(
            &mut out,
            "editorSessionSetSelection",
            // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
            &take_string(unsafe {
                session::writer_core_editor_session_set_selection(sid, 1, 2, revision)
            }),
        ),
        "newRevision",
    );
    revision = num_at(
        &note(
            &mut out,
            "editorSessionDeleteSurrounding",
            // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
            &take_string(unsafe {
                session::writer_core_editor_session_delete_surrounding(
                    sid,
                    0,
                    1,
                    2,
                    2,
                    cstr("Delete").as_ptr(),
                    revision,
                )
            }),
        ),
        "newRevision",
    );
    revision = num_at(
        &note(
            &mut out,
            "editorSessionUndo",
            // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
            &take_string(unsafe { session::writer_core_editor_session_undo(sid, revision) }),
        ),
        "newRevision",
    );
    revision = num_at(
        &note(
            &mut out,
            "editorSessionRedo",
            // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
            &take_string(unsafe { session::writer_core_editor_session_redo(sid, revision) }),
        ),
        "newRevision",
    );
    let _ = note(
        &mut out,
        "editorSessionPreviousGraphemeBoundary",
        // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_previous_grapheme_boundary(sid, 2)
        }),
    );
    let _ = note(
        &mut out,
        "editorSessionNextGraphemeBoundary",
        // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe { session::writer_core_editor_session_next_grapheme_boundary(sid, 2) }),
    );
    let composed = note(
        &mut out,
        "editorSessionBeginComposition",
        // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_begin_composition(sid, 0, 0, revision)
        }),
    );
    let cid = first_num(
        &composed,
        &["compositionSession.sessionId", "composition.sessionId"],
    );
    let cbase = first_num(
        &composed,
        &[
            "compositionSession.baseRevision",
            "composition.baseRevision",
        ],
    );
    let mut cgen = first_num(
        &composed,
        &["compositionSession.generation", "composition.generation"],
    );
    revision = num_at(&composed, "newRevision");
    let updated = note(
        &mut out,
        "editorSessionUpdateComposition",
        // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_update_composition(
                sid,
                cid,
                cgen,
                cstr("你好").as_ptr(),
                2,
                revision,
            )
        }),
    );
    // update 会推进 composition generation，后续调用必须用最新的。
    cgen = first_num(
        &updated,
        &["composition.generation", "compositionSession.generation"],
    );
    let _ = note(
        &mut out,
        "editorSessionCompositionMoveGraphemeRight",
        // SAFETY: sid/cid/cgen 是合法会话/composition 参数，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_composition_move_grapheme_right(
                sid, cid, cgen, revision,
            )
        }),
    );
    let _ = note(
        &mut out,
        "editorSessionCompositionMoveGraphemeLeft",
        // SAFETY: sid/cid/cgen 是合法会话/composition 参数，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_composition_move_grapheme_left(
                sid, cid, cgen, revision,
            )
        }),
    );
    let _ = note(
        &mut out,
        "editorSessionCommitText",
        // SAFETY: 测试持有的合法 C 串指针，返回值由 take_string 立即复制并释放。
        &take_string(unsafe {
            session::writer_core_editor_session_commit_text(
                sid,
                0,
                0,
                cstr("你好").as_ptr(),
                6,
                6,
                cid,
                cbase,
                cgen,
                cstr("ImeComposition").as_ptr(),
                revision,
            )
        }),
    );
    let _ = note(
        &mut out,
        "editorSessionGetText",
        // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe { session::writer_core_editor_session_get_text(sid) }),
    );
    let _ = note(
        &mut out,
        "editorSessionGetRevision",
        // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe { session::writer_core_editor_session_get_revision(sid) }),
    );
    let _ = note(
        &mut out,
        "editorSessionClose",
        // SAFETY: sid 是合法会话 id，返回值由 take_string 立即复制并释放。
        &take_string(unsafe { session::writer_core_editor_session_close(sid) }),
    );

    // ── 基线比对 ──
    const FIXTURE: &str = include_str!("fixtures/harmony_wire_contract.json");
    let rendered = serde_json::to_string_pretty(&out).expect("序列化基线") + "\n";
    if std::env::var("WRITE_HARMONY_WIRE_CONTRACT").is_ok() {
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/harmony_wire_contract.json"
            ),
            &rendered,
        )
        .expect("写基线失败");
        return;
    }
    let expected: Baseline = serde_json::from_str(FIXTURE).expect("基线 JSON 解析失败");
    let mut diff: Vec<String> = Vec::new();
    for (endpoint, node) in &out {
        match expected.get(endpoint) {
            None => diff.push(format!("  + {endpoint}: {}", render(node))),
            Some(e) if e != node => diff.push(format!(
                "  ~ {endpoint}\n      got     = {}\n      fixture = {}",
                render(node),
                render(e)
            )),
            Some(_) => {}
        }
    }
    for endpoint in expected.keys() {
        if !out.contains_key(endpoint) {
            diff.push(format!("  - {endpoint} 已从 Core 输出消失"));
        }
    }
    assert!(
        diff.is_empty(),
        "Harmony 线路契约基线不一致（重新生成见文件头注释）:\n{}",
        diff.join("\n")
    );
}
