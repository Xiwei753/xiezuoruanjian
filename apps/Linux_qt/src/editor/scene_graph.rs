use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGOpacityNode>
    #include <QtQuick/QSGImageNode>
    #include <QtGui/QColor>
    #include <QDebug>

    static QSGNode *child_at(QSGNode *root, int index) {
        return root && index >= 0 && index < root->childCount()
            ? root->childAtIndex(index)
            : nullptr;
    }

    // Four-layer scene graph layout:
    //   child[0] = QSGImageNode       — static text texture
    //   child[1] = QSGTransformNode   — text animation layer
    //   child[2] = QSGTransformNode   — selection / preedit layer
    //   child[3] = QSGTransformNode   — cursor layer (QSGOpacityNode > QSGImageNode)

    static const int LAYER_STATIC_TEXT   = 0;
    static const int LAYER_ANIMATION    = 1;
    static const int LAYER_SELECTION    = 2;
    static const int LAYER_CURSOR       = 3;
    static const int LAYER_COUNT        = 4;

    void ensure_four_layer_nodes(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Remove any extra children beyond LAYER_COUNT
        while (root->childCount() > LAYER_COUNT) {
            QSGNode *extra = child_at(root, root->childCount() - 1);
            root->removeChildNode(extra);
            delete extra;
        }

        // Ensure child[0] is QSGImageNode (static text)
        QSGImageNode *staticNode = nullptr;
        if (root->childCount() > 0) {
            staticNode = dynamic_cast<QSGImageNode*>(child_at(root, 0));
        }
        if (!staticNode) {
            if (root->childCount() > 0) {
                QSGNode *old = child_at(root, 0);
                root->removeChildNode(old);
                delete old;
            }
            staticNode = item->window()->createImageNode();
            staticNode->setFiltering(QSGTexture::Nearest);
            staticNode->setOwnsTexture(true);
            root->prependChildNode(staticNode);
        }

        // Ensure child[1..3] are QSGTransformNode layers
        for (int i = 1; i < LAYER_COUNT; i++) {
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
                // Append at the right position
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

pub fn ensure_four_layer_nodes(root_raw: *mut std::ffi::c_void, item_ptr: *mut std::ffi::c_void) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        ensure_four_layer_nodes(
            static_cast<QSGTransformNode*>(root_raw), item_ptr
        );
    })
}

pub fn update_texture_node(
    old_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    image: &qmetaobject::QImage,
    src_x: f64,
    src_y: f64,
    src_w: f64,
    src_h: f64,
    dest_y: f64,
    dest_h: f64,
    dpr: f64,
) -> *mut std::ffi::c_void {
    let img_ptr = image as *const qmetaobject::QImage;
    cpp!(unsafe [
        old_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        img_ptr as "QImage*",
        src_x as "double", src_y as "double",
        src_w as "double", src_h as "double",
        dest_y as "double", dest_h as "double",
        dpr as "double"
    ] -> *mut std::ffi::c_void as "QSGNode*" {
        auto *root = static_cast<QSGTransformNode*>(old_raw);
        if (!root) {
            root = new QSGTransformNode;
        }

        // Ensure the four-layer structure
        ensure_four_layer_nodes(root, item_ptr);

        // Update child[0] = static text QSGImageNode
        QSGImageNode *imgNode = static_cast<QSGImageNode*>(child_at(root, 0));
        if (!imgNode) {
            imgNode = item_ptr->window()->createImageNode();
            imgNode->setFiltering(QSGTexture::Nearest);
            imgNode->setOwnsTexture(true);
            root->prependChildNode(imgNode);
        }

        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);

        double phys_src_x = src_x * dpr;
        double phys_src_y = src_y * dpr;
        double phys_src_w = src_w * dpr;
        double phys_src_h = src_h * dpr;

        double phys_img_w = static_cast<double>(img_ptr->width());
        double phys_img_h = static_cast<double>(img_ptr->height());

        if (phys_src_y < 0.0) phys_src_y = 0.0;
        if (phys_src_y + phys_src_h > phys_img_h) {
            if (phys_src_h > phys_img_h) phys_src_h = phys_img_h;
            if (phys_src_y + phys_src_h > phys_img_h) phys_src_y = phys_img_h - phys_src_h;
        }
        if (phys_src_x < 0.0) phys_src_x = 0.0;
        if (phys_src_x + phys_src_w > phys_img_w) {
            if (phys_src_w > phys_img_w) phys_src_w = phys_img_w;
            if (phys_src_x + phys_src_w > phys_img_w) phys_src_x = phys_img_w - phys_src_w;
        }

        imgNode->setSourceRect(phys_src_x, phys_src_y, phys_src_w, phys_src_h);
        QSGTexture *tex = item_ptr->window()->createTextureFromImage(*img_ptr);
        tex->setFiltering(QSGTexture::Nearest);
        imgNode->setTexture(tex);
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);

        static bool logged_once = false;
        if (!logged_once && qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG")) {
            logged_once = true;
            qDebug("update_texture_node: dpr=%.2f img=%dx%d texSize=%dx%d srcRect=(%.1f,%.1f %.1fx%.1f) destRect=(0,%.1f %.1fx%.1f)",
                dpr, img_ptr->width(), img_ptr->height(),
                tex->textureSize().width(), tex->textureSize().height(),
                phys_src_x, phys_src_y, phys_src_w, phys_src_h,
                dest_y, item_ptr->width(), dest_h);
        }

        return root;
    })
}

pub fn update_source_rect(
    old_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    src_x: f64,
    src_y: f64,
    src_w: f64,
    src_h: f64,
    dest_y: f64,
    dest_h: f64,
    dpr: f64,
) {
    cpp!(unsafe [
        old_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        src_x as "double", src_y as "double",
        src_w as "double", src_h as "double",
        dest_y as "double", dest_h as "double",
        dpr as "double"
    ] {
        auto *root = static_cast<QSGTransformNode*>(old_raw);
        if (!root || root->childCount() == 0) return;
        auto *imgNode = static_cast<QSGImageNode*>(child_at(root, 0));
        if (!imgNode) return;

        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);

        double phys_src_x = src_x * dpr;
        double phys_src_y = src_y * dpr;
        double phys_src_w = src_w * dpr;
        double phys_src_h = src_h * dpr;

        if (imgNode->texture()) {
            QSize texSize = imgNode->texture()->textureSize();
            double phys_img_w = static_cast<double>(texSize.width());
            double phys_img_h = static_cast<double>(texSize.height());

            if (phys_src_y < 0.0) phys_src_y = 0.0;
            if (phys_src_y + phys_src_h > phys_img_h) {
                if (phys_src_h > phys_img_h) phys_src_h = phys_img_h;
                if (phys_src_y + phys_src_h > phys_img_h) phys_src_y = phys_img_h - phys_src_h;
            }
            if (phys_src_x < 0.0) phys_src_x = 0.0;
            if (phys_src_x + phys_src_w > phys_img_w) {
                if (phys_src_w > phys_img_w) phys_src_w = phys_img_w;
                if (phys_src_x + phys_src_w > phys_img_w) phys_src_x = phys_img_w - phys_src_w;
            }
        }

        imgNode->setSourceRect(phys_src_x, phys_src_y, phys_src_w, phys_src_h);
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    })
}

/// Update the cursor node in the cursor layer (child[3]).
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
/// Incremental update: reuses existing nodes, only updates rect/opacity/texture.
/// If glyph_count > existing child count, new nodes are appended.
/// If glyph_count < existing child count, excess nodes are removed.
pub fn update_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyph_count: i32,
    glyph_data: *const f64,
    images: *const *const qmetaobject::QImage,
    texture_changed: *const bool,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        glyph_count as "int",
        glyph_data as "const double*",
        images as "QImage**",
        texture_changed as "const bool*"
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

        // Each glyph: 6 doubles = x, y, w, h, opacity, baselineY
        for (int i = 0; i < glyph_count; i++) {
            const double *d = glyph_data + i * 6;
            double gx = d[0], gy = d[1], gw = d[2], gh = d[3], gopacity = d[4];

            QSGOpacityNode *opNode = nullptr;
            QSGImageNode *imgNode = nullptr;

            if (i < animLayer->childCount()) {
                // Reuse existing node — only update opacity, rect, texture
                opNode = dynamic_cast<QSGOpacityNode*>(child_at(animLayer, i));
                if (opNode && opNode->childCount() > 0) {
                    imgNode = static_cast<QSGImageNode*>(opNode->firstChild());
                }
            } else {
                // Create new node
                opNode = new QSGOpacityNode;
                animLayer->appendChildNode(opNode);

                imgNode = item_ptr->window()->createImageNode();
                imgNode->setFiltering(QSGTexture::Nearest);
                imgNode->setOwnsTexture(true);
                opNode->appendChildNode(imgNode);
            }

            if (!opNode || !imgNode) continue;

            opNode->setOpacity(static_cast<float>(gopacity));

            imgNode->setRect(static_cast<qreal>(gx), static_cast<qreal>(gy),
                            static_cast<qreal>(gw), static_cast<qreal>(gh));

            // Only update texture when it has changed (first frame or re-prepared)
            bool needTextureUpdate = (texture_changed && texture_changed[i]);
            if (needTextureUpdate && images && images[i]) {
                QSGTexture *tex = item_ptr->window()->createTextureFromImage(*images[i]);
                tex->setFiltering(QSGTexture::Nearest);
                imgNode->setTexture(tex);
            }

            imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }

        if (glyph_count > 0) {
            animLayer->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }
    })
}

/// Clear the animation layer (child[1]) — remove all animated glyph nodes.
pub fn clear_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root) return;

        QSGTransformNode *animLayer = dynamic_cast<QSGTransformNode*>(child_at(root, 1));
        if (!animLayer) return;

        while (animLayer->childCount() > 0) {
            QSGNode *child = animLayer->firstChild();
            animLayer->removeChildNode(child);
            delete child;
        }
    })
}
