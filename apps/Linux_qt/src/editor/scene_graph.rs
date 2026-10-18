use crate::sujian_editor_item::qt_text_node::AnimationClipRect;
use crate::sujian_editor_item::text_decorations::DecorationParagraphGeometry;
use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGOpacityNode>
    #include <QtQuick/QSGImageNode>
    #include <QtQuick/QSGFlatColorMaterial>
    #include <QtQuick/QSGGeometry>
    #include <QtQuick/QSGGeometryNode>
    #include <QtGui/QColor>
    #include <QtGui/QMatrix4x4>
    #include <QtCore/QHash>
    #include <QtCore/QPointF>
    #include <QtCore/QSet>
    #include <QtCore/QVector>
    #include <QDebug>
    #include <algorithm>
    #include <climits>
    #include <cmath>
    #include <iterator>
    #include <utility>
    #include <vector>

    static QSGNode *child_at(QSGNode *root, int index) {
        return root && index >= 0 && index < root->childCount()
            ? root->childAtIndex(index)
            : nullptr;
    }

    // Five-layer scene graph layout (Issue #951):
    //   child[0] = QSGTransformNode   — static text layer (wraps QSGTextNode, Qt 6.7+ public API)
    //   child[1] = AnimationLayerNode — text animation layer (owns GPU texture cache)
    //   child[2] = QSGTransformNode   — selection / preedit layer
    //   child[3] = QSGTransformNode   — text decoration layer (error/warning waves)
    //   child[4] = QSGTransformNode   — cursor layer (QSGOpacityNode > QSGImageNode)

    static const int LAYER_STATIC_TEXT   = 0;
    static const int LAYER_ANIMATION    = 1;
    static const int LAYER_SELECTION    = 2;
    static const int LAYER_DECORATION     = 3;
    static const int LAYER_CURSOR       = 4;
    static const int LAYER_COUNT        = 5;

    // Issue #658 评论 5630650436: GPU texture 生命周期从全局 static cache 改为
    // AnimationLayerNode 自身持有 m_texture_cache。
    // 同一 LineSnapshotId 拆成多个 slice 时，后一个 node 不会删前一个 node 正在引用的 texture；
    // 因为 texture 由各自所在的 AnimationLayerNode 独立管理。
    // 每帧传 active snapshot ids；所有 node rebind 后统一 sweep 未使用的 texture。
    class AnimationLayerNode : public QSGTransformNode {
    public:
        QHash<quint64, QSGTexture*> m_texture_cache;
        QVector<quint64> m_active_snapshot_ids;

        // Issue #904 复核 5: prepare/commit 之间的暂存状态。
        // m_prepared_created 只放本次新创建、尚未进入 m_texture_cache 的纹理。
        QHash<quint64, QSGTexture*> m_prepared_created;
        QVector<QSGTexture*> m_prepared_resolved;
        QVector<double> m_prepared_glyph_data;
        QVector<double> m_prepared_source_rects;
        QVector<quint64> m_prepared_snapshot_ids;
        bool m_prepared = false;
        // 本帧“成立但没有可见 glyph”的预备结果（glyph_count == 0，如吐字 progress=0）。
        bool m_prepared_empty = false;

        // 增量更新：标记本帧活跃的 snapshot id，sweep 不再活跃的 texture。
        // cache miss 时 createTextureFromImage 并存入 cache。
        // 顺序：先删多余 child → 重绑/新增 node → 最后 sweep 旧 texture，
        // 保证没有 QSGImageNode 仍引用待删除的 texture。
        //
        // Issue #904 复核 5: 动画层与本帧静态孔洞必须原子交接，所以把原来的
        // 单次 updateTextures 拆成 prepare / commit / discard 三个阶段，
        // 全部在同一个 updatePaintNode 内完成：
        //
        //   prepareTextures  —— 只检查/预备本帧 QSGTexture（不改动当前子节点、
        //                       不改动 m_texture_cache）；
        //   commitPrepared   —— 只有静态层也提交成功后才真正替换本帧子节点与
        //                       texture cache；
        //   discardPrepared  —— 丢弃预备结果（释放刚建的纹理），旧动画节点不受影响。
        //
        // 全有或全无：任一 glyph 拿不到有效纹理（images 为空 /
        // createTextureFromImage 失败，item/window 缺失）就整体失败。
        // 返回 -1 表示“本帧动画层不成立”，>=0 表示预备成功的 glyph 数。
        int prepareTextures(
            QQuickItem *item,
            int glyph_count,
            const double *glyph_data,
            QImage **images,
            const double *source_rects,
            const quint64 *snapshot_ids
        ) {
            discardPrepared();
            if (glyph_count <= 0) {
                // 本帧应当没有可见动画像素（如吐字 progress=0）。仍然算“动画层成立”：
                // commit 时把子节点清空并 sweep，孔洞由调用方按同一事实挖出。
                m_prepared_empty = true;
                return 0;
            }
            if (!item || !item->window()) return -1;
            m_item_window = item->window();

            QVector<QSGTexture*> resolved(glyph_count, nullptr);
            for (int i = 0; i < glyph_count; i++) {
                quint64 snapId = snapshot_ids ? snapshot_ids[i] : 0;
                QSGTexture *tex = m_texture_cache.value(snapId, nullptr);
                if (!tex) {
                    tex = m_prepared_created.value(snapId, nullptr);
                }
                if (!tex) {
                    if (!images || !images[i]) {
                        discardPrepared();
                        return -1;
                    }
                    tex = item->window()->createTextureFromImage(*images[i]);
                    if (!tex) {
                        discardPrepared();
                        return -1;
                    }
                    tex->setFiltering(QSGTexture::Linear);
                    m_prepared_created.insert(snapId, tex);
                }
                resolved[i] = tex;
            }

            m_prepared_resolved = resolved;
            m_prepared_glyph_data.clear();
            for (int i = 0; i < glyph_count * 5; i++) {
                m_prepared_glyph_data.append(glyph_data[i]);
            }
            m_prepared_source_rects.clear();
            for (int i = 0; i < glyph_count * 4; i++) {
                m_prepared_source_rects.append(source_rects[i]);
            }
            m_prepared_snapshot_ids.clear();
            for (int i = 0; i < glyph_count; i++) {
                m_prepared_snapshot_ids.append(snapshot_ids ? snapshot_ids[i] : 0);
            }
            m_prepared = true;
            return glyph_count;
        }

        // 真正把预备好的本帧动画替换进场景图。只有调用方确认静态层也已提交成功
        // 时才会调用。返回 false 表示本次没有任何预备结果可提交（不是成功）。
        bool commitPrepared() {
            if (!m_prepared && !m_prepared_empty) return false;
            const int glyph_count = m_prepared_snapshot_ids.size();
            m_prepared = false;
            m_prepared_empty = false;

            // 提交纹理到 cache（回滚点已过）。
            for (auto it = m_prepared_created.begin(); it != m_prepared_created.end(); ++it) {
                m_texture_cache.insert(it.key(), it.value());
            }
            m_prepared_created.clear();

            // 标记本帧活跃的 snapshot ids
            m_active_snapshot_ids = m_prepared_snapshot_ids;
            QSet<quint64> active_set(m_active_snapshot_ids.begin(), m_active_snapshot_ids.end());

            // Step 1: Remove excess child nodes (delete nodes referencing old textures)
            while (childCount() > glyph_count) {
                QSGNode *child = child_at(this, childCount() - 1);
                removeChildNode(child);
                delete child;
            }

            // Step 2: Update or create glyph nodes — rebind textures before sweep.
            // m_prepared_resolved[i] 已在 prepare 阶段保证非空。
            for (int i = 0; i < glyph_count; i++) {
                const double *d = m_prepared_glyph_data.constData() + i * 5;
                double gx = d[0], gy = d[1], gw = d[2], gh = d[3], gopacity = d[4];

                const double *sr = m_prepared_source_rects.constData() + i * 4;
                double sx = sr[0], sy = sr[1], sw = sr[2], sh = sr[3];

                QSGOpacityNode *opNode = nullptr;
                QSGImageNode *imgNode = nullptr;

                if (i < childCount()) {
                    opNode = dynamic_cast<QSGOpacityNode*>(child_at(this, i));
                    if (opNode && opNode->childCount() > 0) {
                        imgNode = static_cast<QSGImageNode*>(opNode->firstChild());
                    }
                } else {
                    opNode = new QSGOpacityNode;
                    appendChildNode(opNode);

                    imgNode = m_item_window->createImageNode();
                    imgNode->setFiltering(QSGTexture::Linear);
                    imgNode->setOwnsTexture(false);
                    opNode->appendChildNode(imgNode);
                }

                if (!opNode || !imgNode) continue;

                opNode->setOpacity(static_cast<float>(gopacity));

                imgNode->setRect(static_cast<qreal>(gx), static_cast<qreal>(gy),
                                static_cast<qreal>(gw), static_cast<qreal>(gh));

                if (sw > 0.0 && sh > 0.0) {
                    imgNode->setSourceRect(static_cast<qreal>(sx), static_cast<qreal>(sy),
                                           static_cast<qreal>(sw), static_cast<qreal>(sh));
                } else {
                    imgNode->setSourceRect(0, 0, 0, 0);
                }

                imgNode->setTexture(m_prepared_resolved[i]);
                imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
            }

            // Step 3: Sweep textures not in active_set — safe now, all nodes re/deleted
            auto it = m_texture_cache.begin();
            while (it != m_texture_cache.end()) {
                if (!active_set.contains(it.key())) {
                    delete it.value();
                    it = m_texture_cache.erase(it);
                } else {
                    ++it;
                }
            }

            m_prepared_resolved.clear();
            m_prepared_glyph_data.clear();
            m_prepared_source_rects.clear();
            m_prepared_snapshot_ids.clear();

            markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
            return true;
        }

        // 丢弃预备结果：释放本次新创建的纹理，不动现有子节点 / m_texture_cache。
        void discardPrepared() {
            for (auto it = m_prepared_created.begin(); it != m_prepared_created.end(); ++it) {
                delete it.value();
            }
            m_prepared_created.clear();
            m_prepared_resolved.clear();
            m_prepared_glyph_data.clear();
            m_prepared_source_rects.clear();
            m_prepared_snapshot_ids.clear();
            m_prepared = false;
            m_prepared_empty = false;
        }

        // commit 阶段需要 window 创建 glyph 节点；由 prepare/commit 入口记录。
        QQuickWindow *m_item_window = nullptr;

        // 清空 texture cache 中所有 GPU texture。
        void clearTextureCache() {
            discardPrepared();
            for (auto it = m_texture_cache.begin(); it != m_texture_cache.end(); ++it) {
                delete it.value();
            }
            m_texture_cache.clear();
        }

        // 析构时清理 GPU texture cache，防止窗口关闭/scene graph 失效时泄漏。
        // 子节点由 QSGNode 基类析构负责。
        ~AnimationLayerNode() override {
            clearTextureCache();
        }

        // 清空所有子节点和 texture cache
        void clearAll() {
            while (childCount() > 0) {
                QSGNode *child = firstChild();
                removeChildNode(child);
                delete child;
            }
            clearTextureCache();
        }
    };

    void ensure_five_layer_nodes(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Migrate the previous four-layer layout without replacing its cursor node.
        // In that layout child[3] was the cursor transform; it becomes child[4].
        if (root->childCount() == 4) {
            QSGNode *oldCursorLayer = child_at(root, LAYER_DECORATION);
            if (oldCursorLayer) {
                auto *decorationLayer = new QSGTransformNode;
                root->insertChildNodeBefore(decorationLayer, oldCursorLayer);
            }
        }

        // Remove any extra children beyond LAYER_COUNT
        while (root->childCount() > LAYER_COUNT) {
            QSGNode *extra = child_at(root, root->childCount() - 1);
            root->removeChildNode(extra);
            delete extra;
        }

        // Ensure child[0..4] are all correct layer types.
        // child[0] = QSGTransformNode (static text, wraps QSGTextNode)
        // child[1] = AnimationLayerNode (animation, owns GPU texture cache)
        // child[2] = QSGTransformNode (selection/preedit)
        // child[3] = QSGTransformNode (text decoration)
        // child[4] = QSGTransformNode (cursor)
        for (int i = 0; i < LAYER_COUNT; i++) {
            QSGNode *existing = child_at(root, i);
            bool correct_type = false;
            if (existing) {
                if (i == LAYER_ANIMATION) {
                    correct_type = (dynamic_cast<AnimationLayerNode*>(existing) != nullptr);
                } else {
                    correct_type = (dynamic_cast<QSGTransformNode*>(existing) != nullptr);
                }
            }
            if (!correct_type) {
                if (existing) {
                    root->removeChildNode(existing);
                    delete existing;
                }
                QSGNode *layer;
                if (i == LAYER_ANIMATION) {
                    layer = new AnimationLayerNode;
                } else {
                    layer = new QSGTransformNode;
                }
                if (root->childCount() <= i) {
                    root->appendChildNode(layer);
                } else {
                    root->insertChildNodeBefore(layer, child_at(root, i));
                }
            }
        }

        // Ensure cursor layer has an opacity child with a solid-color rect node
        QSGTransformNode *cursorLayer = dynamic_cast<QSGTransformNode*>(child_at(root, LAYER_CURSOR));
        if (cursorLayer && cursorLayer->childCount() == 0) {
            QSGOpacityNode *opacityNode = new QSGOpacityNode;
            opacityNode->setOpacity(1.0);
            cursorLayer->appendChildNode(opacityNode);
        }
    }

    class TextDecorationParagraphNode : public QSGTransformNode {
    public:
        explicit TextDecorationParagraphNode(quint64 paragraph_id)
            : m_paragraph_id(paragraph_id) {}

        quint64 m_paragraph_id;
    };

    static QSGGeometryNode *create_text_decoration_node(QSGTransformNode *layer, const QColor &color) {
        auto *node = new QSGGeometryNode;
        auto *geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 0);
        geometry->setDrawingMode(QSGGeometry::DrawTriangles);
        node->setGeometry(geometry);
        node->setFlag(QSGNode::OwnsGeometry);

        auto *material = new QSGFlatColorMaterial;
        material->setColor(color);
        material->setFlag(QSGMaterial::Blending);
        node->setMaterial(material);
        node->setFlag(QSGNode::OwnsMaterial);
        node->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        layer->appendChildNode(node);
        return node;
    }

    static QSGGeometryNode *text_decoration_node_at_or_create(
        QSGTransformNode *layer,
        int index,
        const QColor &color
    ) {
        QSGGeometryNode *node = nullptr;
        if (index < layer->childCount()) {
            node = dynamic_cast<QSGGeometryNode*>(child_at(layer, index));
        }
        if (node) return node;

        if (index < layer->childCount()) {
            QSGNode *oldNode = child_at(layer, index);
            layer->removeChildNode(oldNode);
            delete oldNode;
        }
        node = create_text_decoration_node(layer, color);
        if (layer->childCount() > index) {
            layer->removeChildNode(node);
            layer->insertChildNodeBefore(node, child_at(layer, index));
        }
        return node;
    }

    static void update_text_decoration_geometry(
        QSGGeometryNode *node,
        const double *vertices,
        size_t flat_value_count
    ) {
        // Each wave consists of complete triangles; ignore any malformed trailing values.
        size_t triangle_count = flat_value_count / 6;
        const size_t max_triangle_count = static_cast<size_t>(INT_MAX / 3);
        if (triangle_count > max_triangle_count) triangle_count = max_triangle_count;
        const int vertex_count = static_cast<int>(triangle_count * 3);

        QSGGeometry *geometry = node->geometry();
        if (!geometry) {
            geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 0);
            geometry->setDrawingMode(QSGGeometry::DrawTriangles);
            node->setGeometry(geometry);
            node->setFlag(QSGNode::OwnsGeometry);
        }

        const bool reallocated = geometry->vertexCount() != vertex_count;
        bool changed = reallocated;
        if (reallocated) geometry->allocate(vertex_count);

        if (vertex_count > 0) {
            QSGGeometry::Point2D *target = geometry->vertexDataAsPoint2D();
            for (int i = 0; i < vertex_count; i++) {
                const float x = static_cast<float>(vertices[i * 2]);
                const float y = static_cast<float>(vertices[i * 2 + 1]);
                if (reallocated || target[i].x != x || target[i].y != y) {
                    target[i].set(x, y);
                    changed = true;
                }
            }
        }

        if (changed) {
            geometry->markVertexDataDirty();
            node->markDirty(QSGNode::DirtyGeometry);
        }
    }

    static double point_coordinate(const QPointF &point, int axis) {
        return axis == 0 ? point.x() : point.y();
    }

    static std::vector<QPointF> clip_polygon_half_plane(
        const std::vector<QPointF> &input,
        int axis,
        double boundary,
        bool keep_greater
    ) {
        constexpr double EPS = 1e-7;
        std::vector<QPointF> output;
        if (input.empty()) return output;

        auto inside = [axis, boundary, keep_greater, EPS](const QPointF &point) {
            const double value = point_coordinate(point, axis);
            return keep_greater ? value >= boundary - EPS : value <= boundary + EPS;
        };
        auto intersection = [axis, boundary](const QPointF &from, const QPointF &to) {
            const double from_value = point_coordinate(from, axis);
            const double to_value = point_coordinate(to, axis);
            const double denominator = to_value - from_value;
            const double t = std::abs(denominator) < 1e-12
                ? 0.0
                : (boundary - from_value) / denominator;
            return QPointF(
                from.x() + (to.x() - from.x()) * t,
                from.y() + (to.y() - from.y()) * t
            );
        };

        QPointF previous = input.back();
        bool previous_inside = inside(previous);
        for (const QPointF &current : input) {
            const bool current_inside = inside(current);
            if (current_inside != previous_inside) {
                output.push_back(intersection(previous, current));
            }
            if (current_inside) output.push_back(current);
            previous = current;
            previous_inside = current_inside;
        }

        std::vector<QPointF> compact;
        compact.reserve(output.size());
        for (const QPointF &point : output) {
            if (compact.empty()
                || std::abs(compact.back().x() - point.x()) > EPS
                || std::abs(compact.back().y() - point.y()) > EPS) {
                compact.push_back(point);
            }
        }
        if (compact.size() > 1
            && std::abs(compact.front().x() - compact.back().x()) <= EPS
            && std::abs(compact.front().y() - compact.back().y()) <= EPS) {
            compact.pop_back();
        }
        return compact;
    }

    static double polygon_area_twice(const std::vector<QPointF> &polygon) {
        double area = 0.0;
        for (size_t i = 0; i < polygon.size(); i++) {
            const QPointF &a = polygon[i];
            const QPointF &b = polygon[(i + 1) % polygon.size()];
            area += a.x() * b.y() - b.x() * a.y();
        }
        return area;
    }

    static void append_non_degenerate_polygon(
        std::vector<std::vector<QPointF>> &output,
        std::vector<QPointF> polygon
    ) {
        if (polygon.size() >= 3 && std::abs(polygon_area_twice(polygon)) > 1e-10) {
            output.push_back(std::move(polygon));
        }
    }

    // Return the pieces of a convex polygon that lie outside one rectangle.
    static std::vector<std::vector<QPointF>> subtract_decoration_hole(
        const std::vector<QPointF> &polygon,
        double left,
        double top,
        double right,
        double bottom
    ) {
        double min_x = polygon.front().x(), max_x = min_x;
        double min_y = polygon.front().y(), max_y = min_y;
        for (const QPointF &point : polygon) {
            min_x = std::min(min_x, point.x());
            max_x = std::max(max_x, point.x());
            min_y = std::min(min_y, point.y());
            max_y = std::max(max_y, point.y());
        }
        if (max_x <= left || min_x >= right || max_y <= top || min_y >= bottom) {
            return {polygon};
        }

        std::vector<std::vector<QPointF>> outside;
        std::vector<QPointF> remaining = polygon;
        append_non_degenerate_polygon(outside, clip_polygon_half_plane(remaining, 0, left, false));
        remaining = clip_polygon_half_plane(remaining, 0, left, true);
        append_non_degenerate_polygon(outside, clip_polygon_half_plane(remaining, 0, right, false));
        remaining = clip_polygon_half_plane(remaining, 0, right, true);
        append_non_degenerate_polygon(outside, clip_polygon_half_plane(remaining, 1, top, false));
        remaining = clip_polygon_half_plane(remaining, 1, top, true);
        append_non_degenerate_polygon(outside, clip_polygon_half_plane(remaining, 1, bottom, false));
        return outside;
    }

    static std::vector<double> clip_decoration_triangles(
        const double *vertices,
        size_t flat_value_count,
        const double *holes,
        int hole_count
    ) {
        std::vector<double> clipped;
        const size_t triangle_count = flat_value_count / 6;
        for (size_t triangle = 0; triangle < triangle_count; triangle++) {
            const size_t start = triangle * 6;
            std::vector<std::vector<QPointF>> pieces = {{
                QPointF(vertices[start], vertices[start + 1]),
                QPointF(vertices[start + 2], vertices[start + 3]),
                QPointF(vertices[start + 4], vertices[start + 5])
            }};

            for (int hole_index = 0; hole_index < hole_count && !pieces.empty(); hole_index++) {
                const double *hole = holes + hole_index * 4;
                const double left = hole[0];
                const double top = hole[1];
                const double right = left + hole[2];
                const double bottom = top + hole[3];
                if (hole[2] <= 0.0 || hole[3] <= 0.0) continue;

                std::vector<std::vector<QPointF>> next;
                for (const auto &piece : pieces) {
                    std::vector<std::vector<QPointF>> outside =
                        subtract_decoration_hole(piece, left, top, right, bottom);
                    next.insert(next.end(),
                                std::make_move_iterator(outside.begin()),
                                std::make_move_iterator(outside.end()));
                }
                pieces = std::move(next);
            }

            for (const auto &piece : pieces) {
                for (size_t i = 1; i + 1 < piece.size(); i++) {
                    const size_t max_vertices = static_cast<size_t>(INT_MAX / 3) * 3;
                    if (clipped.size() / 2 + 3 > max_vertices) return clipped;
                    const QPointF points[] = {piece[0], piece[i], piece[i + 1]};
                    for (const QPointF &point : points) {
                        clipped.push_back(point.x());
                        clipped.push_back(point.y());
                    }
                }
            }
        }
        return clipped;
    }
}}

/// Issue #677 评论 5653315696: 取得或创建 editor root 节点。
/// 若 `root_raw` 为 null（第一帧或旧节点被回收），创建新的 `QSGTransformNode` 作为
/// editor root 并返回其指针；若非 null，直接返回它。
///
/// 这把"根节点是否为空"的判断从 `ensure_five_layer_nodes` 收口到此函数，
/// 使 `update_paint_node` 不再因为第一帧 root 为 null 就整帧跳过渲染。
///
/// 在 threaded render loop 下，此函数在 render thread 上的 updatePaintNode() 中调用。
pub fn ensure_editor_root(root_raw: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    // SAFETY: root_raw 来自 Qt scene graph 的 SGNode::into_raw()，可能是 null（第一帧）；
    // 返回值要么是原非 null 指针，要么是新分配的 QSGTransformNode，由 QQuickItem::updatePaintNode
    // 契约保证返回的节点有效并由 Qt 持有所有权。
    cpp!(unsafe [root_raw as "QSGNode*"] -> *mut std::ffi::c_void as "QSGNode*" {
        QSGNode *root = static_cast<QSGNode*>(root_raw);
        if (!root) {
            root = new QSGTransformNode;
        }
        return root;
    })
}

/// 确保场景图五层结构——从上到下：staticText(0), animatedText(1), selection/preedit(2),
/// textDecoration(3), cursor(4)。
/// 每层由 QSGOpacityNode 包裹，支持独立透明度控制。
/// 在 threaded render loop 下，此函数在 render thread 上的 updatePaintNode() 中调用。
///
/// Issue #677 评论 5653315696: 调用方须先通过 `ensure_editor_root` 保证 `root_raw` 非 null，
/// 本函数不再承担"根节点是否为空的判断"。C++ 内部保留 `if (!root || !item) return;` 仅作
/// 防御性编程，以保护 prepare_animation_layer / commit_animation_layer /
/// update_cursor_node / update_selection_preedit_layer / update_text_decoration_layer 等内部调用者传入可能为 null 的 root。
pub fn ensure_five_layer_nodes(root_raw: *mut std::ffi::c_void, item_ptr: *mut std::ffi::c_void) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; called from render thread in updatePaintNode() (threaded render loop) or GUI thread; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        ensure_five_layer_nodes(
            static_cast<QSGTransformNode*>(root_raw), item_ptr
        );
    })
}

/// 更新光标节点（child[LAYER_CURSOR] 层内）。
/// Creates/updates a solid-color rectangle at (x, y) with given width/height and opacity.
/// Issue #727 评论 5755858583 问题1: cursor_y 是文档坐标，cursor layer 的 QSGTransformNode
/// 统一做 translate(0, -scroll_y)，和正文/动画层一致，不再在 Rust 侧提前减 scroll_y。
pub fn update_cursor_node(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    cursor_x: f64,
    cursor_y: f64,
    cursor_w: f64,
    cursor_h: f64,
    opacity: f64,
    color_str: *const u8,
    color_len: usize,
    scroll_y: f64,
) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; called from render thread in updatePaintNode() (threaded render loop) or GUI thread; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        cursor_x as "double",
        cursor_y as "double",
        cursor_w as "double",
        cursor_h as "double",
        opacity as "double",
        color_str as "const char*",
        color_len as "size_t",
        scroll_y as "double"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root || !item_ptr) return;

        // Ensure five-layer structure
        ensure_five_layer_nodes(root, item_ptr);

        // Get cursor layer (child[LAYER_CURSOR])
        QSGTransformNode *cursorLayer = dynamic_cast<QSGTransformNode*>(child_at(root, LAYER_CURSOR));
        if (!cursorLayer) return;

        // Issue #727 评论 5755858583 问题1: cursor layer 和正文/动画层使用相同的
        // translate(0, -scroll_y) 矩阵，cursor 保持文档坐标，由 QSGTransformNode
        // 统一做视口变换。
        QMatrix4x4 cursorMatrix;
        cursorMatrix.translate(0, -static_cast<qreal>(scroll_y));
        cursorLayer->setMatrix(cursorMatrix);
        cursorLayer->markDirty(QSGNode::DirtyMatrix);

        // Get or create opacity node
        QSGOpacityNode *opacityNode = nullptr;
        if (cursorLayer->childCount() > 0) {
            opacityNode = dynamic_cast<QSGOpacityNode*>(cursorLayer->firstChild());
        }
        if (!opacityNode) {
            opacityNode = new QSGOpacityNode;
            cursorLayer->appendChildNode(opacityNode);
        }
        opacityNode->setOpacity(static_cast<float>(opacity));

        // Get or create the solid-color image node under opacity
        QSGImageNode *rectNode = nullptr;
        if (opacityNode->childCount() > 0) {
            rectNode = static_cast<QSGImageNode*>(opacityNode->firstChild());
        }
        if (!rectNode) {
            rectNode = item_ptr->window()->createImageNode();
            rectNode->setFiltering(QSGTexture::Nearest);
            rectNode->setOwnsTexture(true);
            opacityNode->appendChildNode(rectNode);
        }

        // Create a 1x1 solid color QImage and scale it
        QString qColorStr = QString::fromUtf8(color_str, static_cast<int>(color_len));
        QColor color(qColorStr);
        QImage cursorImg(static_cast<int>(cursor_w + 0.5), static_cast<int>(cursor_h + 0.5), QImage::Format_RGBA8888);
        cursorImg.fill(color);

        rectNode->setRect(static_cast<qreal>(cursor_x), static_cast<qreal>(cursor_y),
                          static_cast<qreal>(cursor_w), static_cast<qreal>(cursor_h));
        QSGTexture *tex = item_ptr->window()->createTextureFromImage(cursorImg);
        tex->setFiltering(QSGTexture::Nearest);
        rectNode->setTexture(tex);
        rectNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    })
}

/// Update the animation layer (child[1]) with a list of animated glyph quads.
/// Each glyph uses a shared line texture with sourceRect for UV clipping.
/// Incremental update: reuses existing nodes, only updates rect/opacity/texture/sourceRect.
/// If glyph_count > existing child count, new nodes are appended.
/// If glyph_count < existing child count, excess nodes are removed.
///
/// Prepare the animation layer (child[1]) textures for this frame.
///
/// Issue #904 复核 5: 两阶段提交的第一阶段。只检查/预备本帧 QSGTexture，
/// **不改动当前子节点与 texture cache**。返回 -1 表示本帧动画层不成立
/// （root / 动画层节点缺失、纹理创建失败）；>=0 表示预备成功的 glyph 数
/// （0 表示“本帧应当没有可见动画像素”，仍是成立的一帧）。
/// 只有调用方确认静态层也已提交成功时，才调用 [`commit_animation_layer`]；
/// 否则调用 [`discard_animation_layer`] 丢弃预备结果，旧视觉不受影响。
///
/// Issue #658 评论 5630650436: GPU texture cache 由 AnimationLayerNode 自身持有，
/// 不再使用全局 static cache。同一 LineSnapshotId 拆成多个 slice 时，
/// 后一个 node 不会删前一个 node 正在引用的 texture。
/// 每帧传 active snapshot ids；所有 node rebind 后统一 sweep 未使用的 texture。
pub fn prepare_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyph_count: i32,
    glyph_data: *const f64,
    images: *const *const qmetaobject::QImage,
    source_rects: *const f64,
    snapshot_ids: *const u64,
) -> i32 {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; called from render thread in updatePaintNode() (threaded render loop) or GUI thread; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        glyph_count as "int",
        glyph_data as "const double*",
        images as "QImage**",
        source_rects as "const double*",
        snapshot_ids as "const quint64*"
    ] -> i32 as "int" {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return -1;

        ensure_five_layer_nodes(root, item_ptr);

        QSGNode *animNode = child_at(root, LAYER_ANIMATION);
        if (!animNode) return -1;

        auto *animLayer = dynamic_cast<AnimationLayerNode*>(animNode);
        if (!animLayer) return -1;

        return animLayer->prepareTextures(
            item_ptr, glyph_count, glyph_data, images, source_rects, snapshot_ids
        );
    })
}

/// Commit the prepared animation layer: replace child nodes / texture cache.
///
/// Issue #904 复核 5: 只有本帧静态层也提交成功时才调用。`scroll_y` 在这里设置
/// 动画层矩阵（与静态层相同的 `translate(0, -scroll_y)`，Issue #715）。
/// 返回 `false` 表示没有可提交的预备结果。
pub fn commit_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    scroll_y: f64,
) -> bool {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; called from render thread in updatePaintNode() (threaded render loop) or GUI thread; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        scroll_y as "double"
    ] -> bool as "bool" {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return false;

        QSGNode *animNode = child_at(root, LAYER_ANIMATION);
        if (!animNode) return false;

        auto *animLayer = dynamic_cast<AnimationLayerNode*>(animNode);
        if (!animLayer) return false;

        // Issue #715: 动画层和静态正文层使用相同的 translate(0, -scroll_y) 矩阵，
        // 动画 glyph 保持文档坐标，由 QSGTransformNode 统一做视口变换。
        // 之前静态层有滚动矩阵、动画层没有，导致切章继承非零 contentY 后
        // 事务仍正常完成、动画却画到错误 Y 坐标。
        QMatrix4x4 animMatrix;
        animMatrix.translate(0, -static_cast<qreal>(scroll_y));
        animLayer->setMatrix(animMatrix);
        animLayer->markDirty(QSGNode::DirtyMatrix);

        // Issue #904 复核 6：只有真的有预备结果（m_prepared / m_prepared_empty）
        // 才算提交成功；否则返回 false，避免"调用了但没提交仍报成功"。
        return animLayer->commitPrepared();
    })
}

/// Discard a prepared-but-not-committed animation layer (frees newly created textures).
pub fn discard_animation_layer(root_raw: *mut std::ffi::c_void, item_ptr: *mut std::ffi::c_void) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; called from render thread in updatePaintNode() (threaded render loop) or GUI thread; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        QSGNode *animNode = child_at(root, LAYER_ANIMATION);
        if (!animNode) return;

        auto *animLayer = dynamic_cast<AnimationLayerNode*>(animNode);
        if (!animLayer) return;

        animLayer->discardPrepared();
    })
}

/// Clear the animation layer (child[1]) — remove all animated glyph nodes and clear GPU texture cache.
pub fn clear_animation_layer(root_raw: *mut std::ffi::c_void, item_ptr: *mut std::ffi::c_void) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        QSGNode *animNode = child_at(root, LAYER_ANIMATION);
        if (!animNode) return;

        auto *animLayer = dynamic_cast<AnimationLayerNode*>(animNode);
        if (!animLayer) return;

        animLayer->clearAll();
    })
}

/// Update the text decoration layer (child[3]) with paragraph-grouped wave geometry.
/// Vertices and static holes use document coordinates. Paragraph offsets are applied
/// by reusable per-paragraph transforms; the decoration layer applies scroll_y.
pub(crate) fn update_text_decoration_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    paragraphs: &[DecorationParagraphGeometry],
    paragraph_y_offsets: &[(u64, f64)],
    static_holes: &[AnimationClipRect],
    scroll_y: f64,
) {
    let paragraph_ids: Vec<u64> = paragraphs.iter().map(|group| group.paragraph_id).collect();
    let mut error_offsets = Vec::with_capacity(paragraphs.len());
    let mut error_lengths = Vec::with_capacity(paragraphs.len());
    let mut error_vertices = Vec::new();
    let mut warning_offsets = Vec::with_capacity(paragraphs.len());
    let mut warning_lengths = Vec::with_capacity(paragraphs.len());
    let mut warning_vertices = Vec::new();
    for group in paragraphs {
        error_offsets.push(error_vertices.len());
        error_lengths.push(group.error_vertices.len());
        error_vertices.extend_from_slice(&group.error_vertices);
        warning_offsets.push(warning_vertices.len());
        warning_lengths.push(group.warning_vertices.len());
        warning_vertices.extend_from_slice(&group.warning_vertices);
    }

    let offset_ids: Vec<u64> = paragraph_y_offsets.iter().map(|(id, _)| *id).collect();
    let offset_values: Vec<f64> = paragraph_y_offsets
        .iter()
        .map(|(_, offset)| *offset)
        .collect();
    let mut hole_values = Vec::with_capacity(static_holes.len() * 4);
    for hole in static_holes {
        hole_values.extend_from_slice(&[hole.x, hole.y, hole.w, hole.h]);
    }

    let paragraph_ids_ptr = paragraph_ids.as_ptr();
    let error_offsets_ptr = error_offsets.as_ptr();
    let error_lengths_ptr = error_lengths.as_ptr();
    let error_vertices_ptr = error_vertices.as_ptr();
    let warning_offsets_ptr = warning_offsets.as_ptr();
    let warning_lengths_ptr = warning_lengths.as_ptr();
    let warning_vertices_ptr = warning_vertices.as_ptr();
    let offset_ids_ptr = offset_ids.as_ptr();
    let offset_values_ptr = offset_values.as_ptr();
    let hole_values_ptr = hole_values.as_ptr();
    let paragraph_count = paragraphs.len();
    let offset_count = paragraph_y_offsets.len();
    let hole_count = static_holes.len() as i32;

    // SAFETY: all packed-array pointers refer to Rust vectors kept alive for this call;
    // root/item are Qt-owned scene graph pointers used on the render thread by the caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        paragraph_ids_ptr as "const quint64*",
        error_offsets_ptr as "const size_t*",
        error_lengths_ptr as "const size_t*",
        error_vertices_ptr as "const double*",
        warning_offsets_ptr as "const size_t*",
        warning_lengths_ptr as "const size_t*",
        warning_vertices_ptr as "const double*",
        offset_ids_ptr as "const quint64*",
        offset_values_ptr as "const double*",
        hole_values_ptr as "const double*",
        paragraph_count as "size_t",
        offset_count as "size_t",
        hole_count as "int",
        scroll_y as "double"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root || !item_ptr) return;

        ensure_five_layer_nodes(root, item_ptr);
        QSGTransformNode *decorationLayer = dynamic_cast<QSGTransformNode*>(
            child_at(root, LAYER_DECORATION)
        );
        if (!decorationLayer) return;

        QMatrix4x4 decorationMatrix;
        decorationMatrix.translate(0, -static_cast<qreal>(scroll_y));
        if (decorationLayer->matrix() != decorationMatrix) {
            decorationLayer->setMatrix(decorationMatrix);
            decorationLayer->markDirty(QSGNode::DirtyMatrix);
        }

        const QColor errorColor(220, 38, 38);
        const QColor warningColor(245, 124, 0);
        QSet<quint64> activeParagraphs;
        for (size_t i = 0; i < paragraph_count; i++) {
            const quint64 paragraphId = paragraph_ids_ptr[i];
            if (activeParagraphs.contains(paragraphId)) continue;

            const double *errorData = error_lengths_ptr[i] > 0
                ? error_vertices_ptr + error_offsets_ptr[i]
                : nullptr;
            const double *warningData = warning_lengths_ptr[i] > 0
                ? warning_vertices_ptr + warning_offsets_ptr[i]
                : nullptr;
            std::vector<double> clippedErrors = clip_decoration_triangles(
                errorData, error_lengths_ptr[i], hole_values_ptr, hole_count
            );
            std::vector<double> clippedWarnings = clip_decoration_triangles(
                warningData, warning_lengths_ptr[i], hole_values_ptr, hole_count
            );
            if (clippedErrors.empty() && clippedWarnings.empty()) continue;

            TextDecorationParagraphNode *paragraphNode = nullptr;
            for (int child = 0; child < decorationLayer->childCount(); child++) {
                auto *candidate = dynamic_cast<TextDecorationParagraphNode*>(
                    child_at(decorationLayer, child)
                );
                if (candidate && candidate->m_paragraph_id == paragraphId) {
                    paragraphNode = candidate;
                    break;
                }
            }
            if (!paragraphNode) {
                paragraphNode = new TextDecorationParagraphNode(paragraphId);
                decorationLayer->appendChildNode(paragraphNode);
            }

            double paragraphOffset = 0.0;
            for (size_t offset = 0; offset < offset_count; offset++) {
                if (offset_ids_ptr[offset] == paragraphId) {
                    paragraphOffset = offset_values_ptr[offset];
                    break;
                }
            }
            QMatrix4x4 paragraphMatrix;
            paragraphMatrix.translate(0, static_cast<qreal>(paragraphOffset));
            if (paragraphNode->matrix() != paragraphMatrix) {
                paragraphNode->setMatrix(paragraphMatrix);
                paragraphNode->markDirty(QSGNode::DirtyMatrix);
            }

            QSGGeometryNode *errorNode = text_decoration_node_at_or_create(
                paragraphNode, 0, errorColor
            );
            QSGGeometryNode *warningNode = text_decoration_node_at_or_create(
                paragraphNode, 1, warningColor
            );
            while (paragraphNode->childCount() > 2) {
                QSGNode *extra = paragraphNode->lastChild();
                paragraphNode->removeChildNode(extra);
                delete extra;
            }

            update_text_decoration_geometry(
                errorNode,
                clippedErrors.empty() ? nullptr : clippedErrors.data(),
                clippedErrors.size()
            );
            update_text_decoration_geometry(
                warningNode,
                clippedWarnings.empty() ? nullptr : clippedWarnings.data(),
                clippedWarnings.size()
            );
            activeParagraphs.insert(paragraphId);
        }

        for (int child = decorationLayer->childCount() - 1; child >= 0; child--) {
            QSGNode *node = child_at(decorationLayer, child);
            auto *paragraphNode = dynamic_cast<TextDecorationParagraphNode*>(node);
            if (!paragraphNode || !activeParagraphs.contains(paragraphNode->m_paragraph_id)) {
                decorationLayer->removeChildNode(node);
                delete node;
            }
        }
    })
}

/// Update the selection/preedit layer (child[2]) with colored rectangles.
/// rect_data: flat array of [x, y, w, h, r, g, b, a, underline] per rect.
/// Reuses existing QSGGeometryNode and only updates vertex data in-place,
/// avoiding per-frame heap allocation of QSGGeometry.
pub fn update_selection_preedit_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    rect_count: i32,
    rect_data: *const f64,
) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; called from render thread in updatePaintNode() (threaded render loop) or GUI thread; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        rect_count as "int",
        rect_data as "const double*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        ensure_five_layer_nodes(root, item_ptr);

        QSGTransformNode *selLayer = dynamic_cast<QSGTransformNode*>(child_at(root, LAYER_SELECTION));
        if (!selLayer) return;

        // Remove excess children
        while (selLayer->childCount() > rect_count) {
            QSGNode *child = selLayer->lastChild();
            selLayer->removeChildNode(child);
            delete child;
        }

        // Each rect: 10 doubles = x, y, w, h, r, g, b, a, underline_flag, _reserved
        for (int i = 0; i < rect_count; i++) {
            const double *d = rect_data + i * 10;
            double rx = d[0], ry = d[1], rw = d[2], rh = d[3];
            int r = static_cast<int>(d[4] * 255);
            int g = static_cast<int>(d[5] * 255);
            int b = static_cast<int>(d[6] * 255);
            int a = static_cast<int>(d[7] * 255);
            bool underline = d[8] > 0.5;

            QColor color(r, g, b, a);

            QSGFlatColorMaterial *matNode = nullptr;
            QSGGeometryNode *geoNode = nullptr;

            if (i < selLayer->childCount()) {
                geoNode = static_cast<QSGGeometryNode*>(child_at(selLayer, i));
                if (geoNode) {
                    matNode = static_cast<QSGFlatColorMaterial*>(geoNode->material());
                }
            } else {
                geoNode = new QSGGeometryNode;
                matNode = new QSGFlatColorMaterial;
                matNode->setFlag(QSGMaterial::Blending);
                geoNode->setMaterial(matNode);
                geoNode->setFlag(QSGNode::OwnsMaterial);
                // Pre-allocate geometry with 4 vertices; will update in-place later
                QSGGeometry *geo = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 4);
                geo->setDrawingMode(QSGGeometry::DrawTriangleStrip);
                geoNode->setGeometry(geo);
                geoNode->setFlag(QSGNode::OwnsGeometry);
                selLayer->appendChildNode(geoNode);
            }

            if (!geoNode || !matNode) continue;

            matNode->setColor(color);

            // Reuse existing geometry — only update vertex data in-place
            QSGGeometry *geo = geoNode->geometry();
            if (!geo || geo->vertexCount() < 4) {
                // Fallback: should not happen, but create if missing
                geo = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 4);
                geo->setDrawingMode(QSGGeometry::DrawTriangleStrip);
                geoNode->setGeometry(geo);
                geoNode->setFlag(QSGNode::OwnsGeometry);
            }

            QSGGeometry::Point2D *v = geo->vertexDataAsPoint2D();
            if (underline) {
                double lineH = 2.0;
                v[0].set(rx, ry + rh - lineH);
                v[1].set(rx + rw, ry + rh - lineH);
                v[2].set(rx, ry + rh);
                v[3].set(rx + rw, ry + rh);
            } else {
                v[0].set(rx, ry);
                v[1].set(rx + rw, ry);
                v[2].set(rx, ry + rh);
                v[3].set(rx + rw, ry + rh);
            }
            geo->markVertexDataDirty();

            geoNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }

        if (rect_count > 0) {
            selLayer->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }
    })
}
