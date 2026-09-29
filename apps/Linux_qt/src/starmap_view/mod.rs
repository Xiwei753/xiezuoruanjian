// =============================================================================
// starmap_view/ — 星图显示层（Linux 平台端所有）
// =============================================================================
//
// 层级：Linux_qt 平台层。
//
// 归属说明：
//   Core 已在「星图 Core 最终收口」中把布局算法、命中测试、运动策略、视口
//   等显示职责全部移出，只保留节点/嵌入的 `position` 数据字段。这些算法原先
//   住在 core/writer_core/src/starmap/ 下，现按 Core 契约归到平台端，本模块
//   就是它们的 Linux 落点。Android/Harmony 各自有等价实现。
//
// 构成：
// - layout_types.rs：纯数据类型（StarMapLayout / StarMapLayoutNode / StarMapViewport）。
// - grid_layout.rs：网格自动布局算法。
// - hittest.rs：节点 AABB 命中测试 + 点到线段距离。
// - edge_render.rs：边箭头/偏移/标签的渲染几何计算。
// - bridge.rs：显示层入口，把上述算法组装成 backend 可直接调的 envelope JSON
//   接口（边渲染/命中测试/网格布局/从图派生布局），并供 starmap_bridge 的
//   `get_starmap_graph_and_layout` 复用 `layout_from_graph`。不碰 Core CRUD。
//
// 坐标约定：
//   全部为星图文档坐标（像素，不含视口滚动偏移），视口变换由 QML 渲染层负责。
// =============================================================================

pub mod bridge;
pub mod edge_render;
pub mod grid_layout;
pub mod hittest;
pub mod layout_types;
