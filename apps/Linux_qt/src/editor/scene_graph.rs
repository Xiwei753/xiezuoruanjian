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
    #include <QDebug>

    static QSGNode *child_at(QSGNode *root, int index) {
        return root && index >= 0 && index < root->childCount()
            ? root->childAtIndex(index)
            : nullptr;
    }

    // Four-layer scene graph layout (Issue #658):
    //   child[0] = QSGTransformNode   — static text layer (wraps QSGTextNode, Qt 6.7+ public API)
    //   child[1] = QSGTransformNode   — text animation layer
    //   child[2] = QSGTransformNode   — selection / preedit layer
    //   child[3] = QSGTransformNode   — cursor layer (QSGOpacityNode > QSGImageNode)

    static const int LAYER_STATIC_TEXT   = 0;
    static const int LAYER_ANIMATION    = 1;
    static const int LAYER_SELECTION    = 2;
    static const int LAYER_CURSOR       = 3;
    static const int LAYER_COUNT        = 4;

    // 修复点 3 (Issue #658 评论 5627327573): render-thread GPU texture cache。
    // 动画期间同一张行纹理只 createTextureFromImage 一次，后续帧只移动/改透明度。
    // key 用 glyph 的 snapshot_id（u64，由 Rust 侧 LineSnapshotId::to_cache_key() 生成）。
    // render thread 单线程，无需锁。QSGTexture 由 cache 统一管理生命周期，node 不 owns texture。
    // clear_animation_layer 不清 cache（texture 可能下帧还用）；release_textures 清指定 id。
    static QHash<quint64, QSGTexture*> g_gpu_texture_cache;

    void ensure_four_layer_nodes(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Remove any extra children beyond LAYER_COUNT
        while (root->childCount() > LAYER_COUNT) {
            QSGNode *extra = child_at(root, root->childCount() - 1);
            root->removeChildNode(extra);
            delete extra;
        }

        // Ensure child[0..3] are all QSGTransformNode layers.
        // child[0] wraps a QSGTextNode (created lazily by qt_text_node module).
        for (int i = 0; i < LAYER_COUNT; i++) {
            QSGTransformNode *layer = nullptr;
            if (root->childCount() > i) {
                layer = dynamic_cast<QSGTransformNode*>(child_at(root, i));
            }
            if (!layer) {
                // Remove wrong-typed node at this slot if present
                if (root->childCount() > i) {
                    QSGNode *old = child_at(root, i);
                    root->removeChildNode(old);
                    delete old;
                }
                layer = new QSGTransformNode;
                // Insert at the right position
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
/// 修复点 3 (Issue #658 评论 5627327573): `snapshot_ids` 传入每个 glyph 的 snapshot_id
/// u64 cache key，用作 C++ 侧 GPU texture cache (QHash<quint64, QSGTexture*>) 的 key。
/// `texture_changed[i]=true` 表示该 node 绑定的 snapshot id 变了（image 内容可能变了），
/// C++ 侧据此决定是否重新 createTextureFromImage；为 false 时直接复用 cache 里的 QSGTexture。
pub fn update_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyph_count: i32,
    glyph_data: *const f64,
    images: *const *const qmetaobject::QImage,
    texture_changed: *const bool,
    source_rects: *const f64,
    snapshot_ids: *const u64,
) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        glyph_count as "int",
        glyph_data as "const double*",
        images as "QImage**",
        texture_changed as "const bool*",
        source_rects as "const double*",
        snapshot_ids as "const quint64*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        ensure_four_layer_nodes(root, item_ptr);

        QSGTransformNode *animLayer = dynamic_cast<QSGTransformNode*>(child_at(root, 1));
        if (!animLayer) return;

        // Remove excess children if glyph count decreased
        while (animLayer->childCount() > glyph_count) {
            QSGNode *child = child_at(animLayer, animLayer->childCount() - 1);
            animLayer->removeChildNode(child);
            delete child;
        }

        // Each glyph: 5 doubles = x, y, w, h, opacity
        // Each sourceRect: 4 doubles = sx, sy, sw, sh
        for (int i = 0; i < glyph_count; i++) {
            const double *d = glyph_data + i * 5;
            double gx = d[0], gy = d[1], gw = d[2], gh = d[3], gopacity = d[4];

            const double *sr = source_rects + i * 4;
            double sx = sr[0], sy = sr[1], sw = sr[2], sh = sr[3];

            quint64 snapId = (snapshot_ids) ? snapshot_ids[i] : 0;

            QSGOpacityNode *opNode = nullptr;
            QSGImageNode *imgNode = nullptr;

            if (i < animLayer->childCount()) {
                opNode = dynamic_cast<QSGOpacityNode*>(child_at(animLayer, i));
                if (opNode && opNode->childCount() > 0) {
                    imgNode = static_cast<QSGImageNode*>(opNode->firstChild());
                }
            } else {
                opNode = new QSGOpacityNode;
                animLayer->appendChildNode(opNode);

                imgNode = item_ptr->window()->createImageNode();
                imgNode->setFiltering(QSGTexture::Linear);
                // 修复点 3: texture 由 g_gpu_texture_cache 统一管理生命周期，node 不 owns。
                imgNode->setOwnsTexture(false);
                opNode->appendChildNode(imgNode);
            }

            if (!opNode || !imgNode) continue;

            opNode->setOpacity(static_cast<float>(gopacity));

            imgNode->setRect(static_cast<qreal>(gx), static_cast<qreal>(gy),
                            static_cast<qreal>(gw), static_cast<qreal>(gh));

            // Set sourceRect for UV clipping from shared line texture
            if (sw > 0.0 && sh > 0.0) {
                imgNode->setSourceRect(static_cast<qreal>(sx), static_cast<qreal>(sy),
                                       static_cast<qreal>(sw), static_cast<qreal>(sh));
            } else {
                imgNode->setSourceRect(static_cast<qreal>(0), static_cast<qreal>(0),
                                       static_cast<qreal>(0), static_cast<qreal>(0));
            }

            // 修复点 3 (Issue #658 评论 5627327573): GPU texture cache。
            // texture_changed[i]=true 表示该 node 绑定的 snapshot id 变了（image 内容可能变了）。
            // - changed: 若 cache 有该 snapId 的旧 texture，delete 旧的；createTextureFromImage 存入 cache。
            // - !changed: 若 cache 有该 snapId 的 texture，直接 setTexture（不重新上传）；
            //   若 cache 没有（首次或被 release 清了），createTextureFromImage 存入。
            // 动画 60 帧同一 snapshot_id：changed=false，cache 命中，只更新 rect/opacity/sourceRect。
            bool changed = (texture_changed && texture_changed[i]);
            QSGTexture *tex = g_gpu_texture_cache.value(snapId, nullptr);
            if (changed) {
                if (tex) {
                    delete tex;
                    g_gpu_texture_cache.remove(snapId);
                    tex = nullptr;
                }
                if (images && images[i]) {
                    tex = item_ptr->window()->createTextureFromImage(*images[i]);
                    tex->setFiltering(QSGTexture::Linear);
                    g_gpu_texture_cache.insert(snapId, tex);
                }
            } else if (!tex) {
                // cache 没有（首次或被 release 清了），创建并存入
                if (images && images[i]) {
                    tex = item_ptr->window()->createTextureFromImage(*images[i]);
                    tex->setFiltering(QSGTexture::Linear);
                    g_gpu_texture_cache.insert(snapId, tex);
                }
            }
            if (tex) {
                imgNode->setTexture(tex);
            }

            imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }

        if (glyph_count > 0) {
            animLayer->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }
    })
}

/// Clear the animation layer (child[1]) — remove all animated glyph nodes and clear GPU texture cache.
pub fn clear_animation_layer(root_raw: *mut std::ffi::c_void, item_ptr: *mut std::ffi::c_void) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        QSGTransformNode *animLayer = dynamic_cast<QSGTransformNode*>(child_at(root, 1));
        if (!animLayer) return;

        // Issue #658: 先删除全部 child node，再清除 GPU texture cache。
        // 不能只删 node 不删 texture，否则 QSGTexture 会泄漏。
        while (animLayer->childCount() > 0) {
            QSGNode *child = animLayer->firstChild();
            animLayer->removeChildNode(child);
            delete child;
        }
        // 清空 g_gpu_texture_cache 中所有 texture，确保"最后一个动画完成/全部取消"
        // 时 GPU cache 不会持续增长。
        for (auto it = g_gpu_texture_cache.begin(); it != g_gpu_texture_cache.end(); ++it) {
            delete it.value();
        }
        g_gpu_texture_cache.clear();
    })
}

/// 修复点 3 (Issue #658 评论 5627327573): 释放 GPU texture cache 中指定 snapshot id 的纹理。
///
/// 在 snapshot/transaction 完成或取消时调用，避免 cache 无限增长。
/// `clear_animation_layer` 不清 cache（texture 可能下帧还用），由本函数按需清理。
/// QSGTexture 只能在 render thread 销毁；本函数在 render thread（updatePaintNode）中调用。
pub fn release_textures(snapshot_ids: *const u64, count: i32) {
    // SAFETY: snapshot_ids 指向 Rust 侧 Vec<u64>，count 为元素数；render thread 单线程访问 file-static cache。
    cpp!(unsafe [
        snapshot_ids as "const quint64*",
        count as "int"
    ] {
        if (!snapshot_ids || count <= 0) return;
        for (int i = 0; i < count; i++) {
            auto it = g_gpu_texture_cache.find(snapshot_ids[i]);
            if (it != g_gpu_texture_cache.end()) {
                delete it.value();
                g_gpu_texture_cache.erase(it);
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
