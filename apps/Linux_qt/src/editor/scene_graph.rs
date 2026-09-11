use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGOpacityNode>
    #include <QtQuick/QSGImageNode>
    #include <QtQuick/QSGFlatColorMaterial>
    #include <QtGui/QColor>
    #include <QtCore/QHash>
    #include <QtCore/QSet>
    #include <QtCore/QVector>
    #include <QDebug>

    static QSGNode *child_at(QSGNode *root, int index) {
        return root && index >= 0 && index < root->childCount()
            ? root->childAtIndex(index)
            : nullptr;
    }

    // Four-layer scene graph layout (Issue #658):
    //   child[0] = QSGTransformNode   — static text layer (wraps QSGTextNode, Qt 6.7+ public API)
    //   child[1] = AnimationLayerNode — text animation layer (owns GPU texture cache)
    //   child[2] = QSGTransformNode   — selection / preedit layer
    //   child[3] = QSGTransformNode   — cursor layer (QSGOpacityNode > QSGImageNode)

    static const int LAYER_STATIC_TEXT   = 0;
    static const int LAYER_ANIMATION    = 1;
    static const int LAYER_SELECTION    = 2;
    static const int LAYER_CURSOR       = 3;
    static const int LAYER_COUNT        = 4;

    // Issue #658 评论 5630650436: GPU texture 生命周期从全局 static cache 改为
    // AnimationLayerNode 自身持有 m_texture_cache。
    // 同一 LineSnapshotId 拆成多个 slice 时，后一个 node 不会删前一个 node 正在引用的 texture；
    // 因为 texture 由各自所在的 AnimationLayerNode 独立管理。
    // 每帧传 active snapshot ids；所有 node rebind 后统一 sweep 未使用的 texture。
    class AnimationLayerNode : public QSGTransformNode {
    public:
        QHash<quint64, QSGTexture*> m_texture_cache;
        QVector<quint64> m_active_snapshot_ids;

        // 增量更新：标记本帧活跃的 snapshot id，sweep 不再活跃的 texture。
        // cache miss 时 createTextureFromImage 并存入 cache。
        // 顺序：先删多余 child → 重绑/新增 node → 最后 sweep 旧 texture，
        // 保证没有 QSGImageNode 仍引用待删除的 texture。
        void updateTextures(
            QQuickItem *item,
            int glyph_count,
            const double *glyph_data,
            QImage **images,
            const double *source_rects,
            const quint64 *snapshot_ids
        ) {
            // 标记本帧活跃的 snapshot ids
            m_active_snapshot_ids.clear();
            if (snapshot_ids) {
                for (int i = 0; i < glyph_count; i++) {
                    m_active_snapshot_ids.append(snapshot_ids[i]);
                }
            }
            QSet<quint64> active_set(m_active_snapshot_ids.begin(), m_active_snapshot_ids.end());

            // Step 1: Remove excess child nodes (delete nodes referencing old textures)
            while (childCount() > glyph_count) {
                QSGNode *child = child_at(this, childCount() - 1);
                removeChildNode(child);
                delete child;
            }

            // Step 2: Update or create glyph nodes — rebind textures before sweep
            for (int i = 0; i < glyph_count; i++) {
                const double *d = glyph_data + i * 5;
                double gx = d[0], gy = d[1], gw = d[2], gh = d[3], gopacity = d[4];

                const double *sr = source_rects + i * 4;
                double sx = sr[0], sy = sr[1], sw = sr[2], sh = sr[3];

                quint64 snapId = snapshot_ids ? snapshot_ids[i] : 0;

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

                    imgNode = item->window()->createImageNode();
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

                // GPU texture cache: cache miss → createTextureFromImage
                QSGTexture *tex = m_texture_cache.value(snapId, nullptr);
                if (!tex && images && images[i]) {
                    tex = item->window()->createTextureFromImage(*images[i]);
                    tex->setFiltering(QSGTexture::Linear);
                    m_texture_cache.insert(snapId, tex);
                }
                if (tex) {
                    imgNode->setTexture(tex);
                }

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

            if (glyph_count > 0) {
                markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
            }
        }

        // 清空 texture cache 中所有 GPU texture。
        void clearTextureCache() {
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

    void ensure_four_layer_nodes(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Remove any extra children beyond LAYER_COUNT
        while (root->childCount() > LAYER_COUNT) {
            QSGNode *extra = child_at(root, root->childCount() - 1);
            root->removeChildNode(extra);
            delete extra;
        }

        // Ensure child[0..3] are all correct layer types.
        // child[0] = QSGTransformNode (static text, wraps QSGTextNode)
        // child[1] = AnimationLayerNode (animation, owns GPU texture cache)
        // child[2] = QSGTransformNode (selection/preedit)
        // child[3] = QSGTransformNode (cursor)
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
}}

/// 确保场景图四层结构——从上到下：staticText(0), animatedText(1), decorations(2), cursor(3)。
/// 每层由 QSGOpacityNode 包裹，支持独立透明度控制。
/// 在 threaded render loop 下，此函数在 render thread 上的 updatePaintNode() 中调用。
pub fn ensure_four_layer_nodes(root_raw: *mut std::ffi::c_void, item_ptr: *mut std::ffi::c_void) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        ensure_four_layer_nodes(
            static_cast<QSGTransformNode*>(root_raw), item_ptr
        );
    })
}

/// 更新光标节点（child[3] 层内）。
/// Creates/updates a solid-color rectangle at (x, y) with given width/height and opacity.
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
) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        cursor_x as "double",
        cursor_y as "double",
        cursor_w as "double",
        cursor_h as "double",
        opacity as "double",
        color_str as "const char*",
        color_len as "size_t"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        // Ensure four-layer structure
        ensure_four_layer_nodes(root, item_ptr);

        // Get cursor layer (child[3])
        QSGTransformNode *cursorLayer = dynamic_cast<QSGTransformNode*>(child_at(root, 3));
        if (!cursorLayer) return;

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
/// Issue #658 评论 5630650436: GPU texture cache 由 AnimationLayerNode 自身持有，
/// 不再使用全局 static cache。同一 LineSnapshotId 拆成多个 slice 时，
/// 后一个 node 不会删前一个 node 正在引用的 texture。
/// 每帧传 active snapshot ids；所有 node rebind 后统一 sweep 未使用的 texture。
pub fn update_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyph_count: i32,
    glyph_data: *const f64,
    images: *const *const qmetaobject::QImage,
    source_rects: *const f64,
    snapshot_ids: *const u64,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        glyph_count as "int",
        glyph_data as "const double*",
        images as "QImage**",
        source_rects as "const double*",
        snapshot_ids as "const quint64*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        ensure_four_layer_nodes(root, item_ptr);

        QSGNode *animNode = child_at(root, LAYER_ANIMATION);
        if (!animNode) return;

        auto *animLayer = dynamic_cast<AnimationLayerNode*>(animNode);
        if (!animLayer) return;

        animLayer->updateTextures(
            item_ptr, glyph_count, glyph_data, images, source_rects, snapshot_ids
        );
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
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        rect_count as "int",
        rect_data as "const double*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        ensure_four_layer_nodes(root, item_ptr);

        QSGTransformNode *selLayer = dynamic_cast<QSGTransformNode*>(child_at(root, 2));
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
