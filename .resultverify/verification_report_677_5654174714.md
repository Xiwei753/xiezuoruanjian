# Issue #677 评论 5654174714 显示层边界收口 — 独立回归验证报告

**验证人**: team-result-verifier (task_id=2)
**验证日期**: 2026-09-13
**验证对象**: PreparedEditorFrame 缓存边界收口（scroll_y/颜色不进入 prepared frame）

---

## 1. 构建与静态检查（退出码）

| 命令 | 退出码 | 结果 |
|------|--------|------|
| `cargo check -p sujian-linux-qt` | 0 | PASS（仅 C++ unused-parameter warnings，非 Rust） |
| `cargo clippy -p sujian-linux-qt -- -D warnings` | 0 | PASS（无 Rust warning） |
| `python3 tools/check_rust_safety_patterns.py .` | 0 | PASS（"Rust 安全策略扫描通过"） |
| `python3 tools/test_check_rust_safety_patterns.py` | 0 | PASS（37 tests OK） |

## 2. 测试运行

| 命令 | 退出码 | 结果 |
|------|--------|------|
| `cargo test -p sujian-linux-qt` | 0 | 3 tests passed：save_empty_requires_explicit_user_clear_flag、without_override_provider_sees_token_missing、sync_profile_override_makes_token_visible_to_provider |

测试无回归。

## 3. 边界确认（逐条对照）

### 3.1 PreparedEditorFrame 边界

| 边界要求 | 确认结果 | 证据 |
|----------|----------|------|
| PreparedEditorFrame 只保存 LayoutSnapshot + selection/preedit 文档坐标几何 | ✅ | render_plan.rs:50-55 — 仅 `layout_snapshot` 和 `selection_preedit` 两字段 |
| SelectionRange 和 PreeditRange 不含 color 字段 | ✅ | render_plan.rs:57-64（SelectionRange: x,y,w,h）、render_plan.rs:66-74（PreeditRange: x,y,w,h,underline） |
| SelectionRange.y 和 PreeditRange.y 是文档坐标（不减 scroll_y） | ✅ | render_plan.rs:60,69 注释明确"文档坐标 y（不减 scroll_y）"；mod.rs:882 `y: line.y`、mod.rs:917 `y: line.y` |
| build_selection_preedit_plan_from_snapshot() 中不计算颜色、不减 scroll_y 到 y | ✅ | mod.rs:826-927 — scroll_y 仅用于视口裁剪判断（行 853 `line_top = line.y - scroll_y` 用于 if 跳过），不写入 SelectionRange.y；无颜色计算 |

### 3.2 轻量状态边界

| 边界要求 | 确认结果 | 证据 |
|----------|----------|------|
| scroll_y/selection/preedit 颜色/cursor 颜色/动画进度不进入 prepared frame | ✅ | PreparedEditorFrame 仅 2 字段（layout_snapshot, selection_preedit）；均不含上述轻量状态 |
| 颜色通过 SelectionPreeditStyle（RenderPlan 的轻量 style 字段）传递 | ✅ | render_plan.rs:101-104（SelectionPreeditStyle 定义）、render_plan.rs:122（RenderPlan.selection_preedit_style 字段） |
| update_paint_node() 读取 current_selection_color 构造 SelectionPreeditStyle | ✅ | qquickitem_impl.rs:171-173 — `SelectionPreeditStyle { selection_color: self.current_selection_color.to_string() }` |

### 3.3 renderer 边界

| 边界要求 | 确认结果 | 证据 |
|----------|----------|------|
| render_selection_preedit_layer() 接收 scroll_y，绘制时做 screen_y = doc_y - scroll_y | ✅ | scene_graph_renderer.rs:268（参数 scroll_y）、行 298 `screen_y = sel.y - scroll_y`、行 305 `screen_y = pre.y - scroll_y` |
| 颜色从 plan.selection_preedit_style.selection_color 读取 | ✅ | scene_graph_renderer.rs:280 `let base_color = &plan.selection_preedit_style.selection_color;` |
| preedit 透明度（0x1A）和 selection 透明度（0x33）在 renderer 计算 | ✅ | scene_graph_renderer.rs:291 `selection_alpha = 0x33 as f64 / 255.0`、行 292 `preedit_alpha = 0x1A as f64 / 255.0` |
| update_paint_node() 不调用任何 layout prepare | ✅ | qquickitem_impl.rs:152-156 — 只读 `self.prepared_frame.as_ref()`，不调用 build_selection_preedit_plan/layout_snapshot/prepare_editor_frame |

### 3.4 set_scroll_y / set_selection_color 路径

| 边界要求 | 确认结果 | 证据 |
|----------|----------|------|
| set_scroll_y() 不触发重新排版（只做 transform/坐标更新） | ✅ | properties.rs:338-349 — 调用 clear_active_text_animations + update_cursor_visual_position + request_frame_update；request_frame_update（mod.rs:759-762）只调 item.update()，不调 prepare_editor_frame |
| set_selection_color() 不触发重新排版（只走 color_only_changed -> request_scene_rebuild） | ✅ | properties.rs:197-212 → color_only_changed（properties.rs:608-612）→ request_scene_rebuild（mod.rs:722-726）只设 scene_dirty + item.update()，不调 prepare_editor_frame |

## 4. 确认无遗漏（对抗性搜索）

| 搜索项 | 结果 | 证据 |
|--------|------|------|
| 构造 `SelectionRange { ... color: ... }` | ✅ 无 | 全仓仅 1 处构造（mod.rs:880），字段为 x,y,w,h，无 color |
| 构造 `PreeditRange { ... color: ... }` | ✅ 无 | 全仓仅 1 处构造（mod.rs:915），字段为 x,y,w,h,underline，无 color |
| 访问 `.color` 在 SelectionRange/PreeditRange 上 | ✅ 无 | `rg "selection_ranges.*\.color\|preedit_ranges.*\.color"` exit=1（零匹配） |
| render thread 调用 build_selection_preedit_plan_from_snapshot | ✅ 无 | 唯一调用在 mod.rs:752（prepare_editor_frame 内，GUI 线程）；scene_graph_renderer.rs:279 仅为注释 |
| render thread 调用 prepare_editor_frame | ✅ 无 | 唯一调用在 mod.rs:714（request_static_repaint 内，GUI 线程）；qquickitem_impl.rs 中仅注释提及 |
| 旧 build_selection_preedit_plan（非 from_snapshot）实际调用 | ✅ 无 | 仅在注释中出现（qquickitem_impl.rs:147, mod.rs:705） |
| prepared_frame 写入点 | ✅ 全在 GUI 线程 | 写入：mod.rs:534(初始化)/713/743/753、properties.rs:631、layout_ops.rs:38；render thread（qquickitem_impl.rs:152,190,192）只读 as_ref/map |

**对抗性探针**: build_render_plan_full（animation_coordinator.rs:1537-1594）正确接收并透传 selection_preedit_style 到 RenderPlan（行 1591），不在内部构造 SelectionRange/PreeditRange。

## 5. 最终 Verdict

### **PASS**

**理由**:
1. 全部 4 个构建/静态检查命令退出码为 0；
2. cargo test 退出码 0，3 个测试全部通过，无回归；
3. PreparedEditorFrame 边界、轻量状态边界、renderer 边界、set_scroll_y/set_selection_color 路径共 11 条边界要求逐条满足；
4. 对抗性搜索确认无遗漏：无带 color 的 SelectionRange/PreeditRange 构造、无 .color 字段访问、render thread 不调用任何 layout prepare、prepared_frame 写入全在 GUI 线程。

修改正确实现了"PreparedEditorFrame 只保存文档坐标几何 + LayoutSnapshot，scroll_y/颜色作为每帧轻量状态通过 RenderPlan style 字段传递，renderer 在绘制时做视口换算"的边界收口目标。
