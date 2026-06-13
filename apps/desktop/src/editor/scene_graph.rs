use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGImageNode>
    #include <QtQuick/QSGRectangleNode>

    // Qt 6.11 compatible helpers — QSGNode no longer has childAt() or
    // insertChildNode(node, index).  Use childAtIndex() and
    // insertChildNodeBefore() / appendChildNode() instead.

    static QSGNode *child_at(QSGNode *root, int index) {
        return root && index >= 0 && index < root->childCount()
            ? root->childAtIndex(index)
            : nullptr;
    }

    static void insert_child_at(QSGNode *root, QSGNode *node, int index) {
        if (!root || !node) return;
        QSGNode *before = child_at(root, index);
        if (before) {
            root->insertChildNodeBefore(node, before);
        } else {
            root->appendChildNode(node);
        }
    }

    // Three-layer scene graph layout:
    //   child[0] = QSGImageNode  — static text texture
    //   child[1] = QSGImageNode  — animation overlay texture
    //   child[2] = QSGRectangleNode — cursor

    // Three-layer scene graph layout:
    //   child[0] = QSGImageNode  — static text texture
    //   child[1] = QSGImageNode  — animation overlay texture
    //   child[2] = QSGRectangleNode — cursor

    void ensure_single_image_node(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Remove any extra children beyond child[0] (overlay, cursor, etc.)
        // This is for SUJIAN_EDITOR_STATIC_ONLY mode — only the static
        // text texture node should exist.
        while (root->childCount() > 1) {
            QSGNode *extra = child_at(root, root->childCount() - 1);
            root->removeChildNode(extra);
            delete extra;
        }

        // Ensure child[0] is QSGImageNode (static text)
        QSGImageNode *staticNode = nullptr;
        if (root->childCount() > 0) {
            staticNode = dynamic_cast<QSGImageNode*>(child_at(root,0));
        }
        if (!staticNode) {
            // Remove wrong-typed child[0] if present
            if (root->childCount() > 0) {
                QSGNode *old = child_at(root,0);
                root->removeChildNode(old);
                delete old;
            }
            staticNode = item->window()->createImageNode();
            staticNode->setFiltering(QSGTexture::Nearest);
            staticNode->setOwnsTexture(true);
            root->prependChildNode(staticNode);
        }
    }

    void ensure_three_layer_nodes(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Ensure child[0] is QSGImageNode (static text)
        QSGImageNode *staticNode = nullptr;
        if (root->childCount() > 0) {
            staticNode = dynamic_cast<QSGImageNode*>(child_at(root,0));
        }
        if (!staticNode) {
            // Remove wrong-typed child[0] if present
            if (root->childCount() > 0) {
                QSGNode *old = child_at(root,0);
                root->removeChildNode(old);
                delete old;
            }
            staticNode = item->window()->createImageNode();
            staticNode->setFiltering(QSGTexture::Nearest);
            staticNode->setOwnsTexture(true);
            root->prependChildNode(staticNode);
        }

        // Ensure child[1] is QSGImageNode (animation overlay)
        QSGImageNode *overlayNode = nullptr;
        if (root->childCount() > 1) {
            overlayNode = dynamic_cast<QSGImageNode*>(child_at(root,1));
        }
        if (!overlayNode) {
            if (root->childCount() > 1) {
                QSGNode *old = child_at(root,1);
                root->removeChildNode(old);
                delete old;
            }
            overlayNode = item->window()->createImageNode();
            overlayNode->setFiltering(QSGTexture::Nearest);
            overlayNode->setOwnsTexture(true);
            int insertIdx = qMin(1, root->childCount());
            insert_child_at(root,overlayNode, insertIdx);
        }

        // Ensure child[2] is QSGRectangleNode (cursor)
        QSGRectangleNode *cursorNode = nullptr;
        if (root->childCount() > 2) {
            cursorNode = dynamic_cast<QSGRectangleNode*>(child_at(root,2));
        }
        if (!cursorNode) {
            if (root->childCount() > 2) {
                QSGNode *old = child_at(root,2);
                root->removeChildNode(old);
                delete old;
            }
            cursorNode = item->window()->createRectangleNode();
            int insertIdx = qMin(2, root->childCount());
            insert_child_at(root,cursorNode, insertIdx);
        }
    }

    void sujian_update_animation_overlay(
        QSGTransformNode *root, QQuickItem *item,
        const QImage *img_ptr,
        double src_x, double src_y, double src_w, double src_h,
        double dest_y, double dest_h, double dpr) {
        if (!root) return;

        // Ensure child[1] exists as QSGImageNode
        QSGImageNode *overlayNode = nullptr;
        if (root->childCount() > 1) {
            overlayNode = dynamic_cast<QSGImageNode*>(child_at(root,1));
        }
        if (!overlayNode) {
            overlayNode = item->window()->createImageNode();
            overlayNode->setFiltering(QSGTexture::Nearest);
            overlayNode->setOwnsTexture(true);
            // Insert at position 1 (after static text child[0], before cursor child[2])
            int insertIdx = qMin(1, root->childCount());
            insert_child_at(root,overlayNode, insertIdx);
        }

        if (!img_ptr || img_ptr->width() == 0 || img_ptr->height() == 0) {
            // No animation — hide overlay and release texture
            overlayNode->setRect(QRectF(0, 0, 0, 0));
            overlayNode->setTexture(nullptr);
            overlayNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
            return;
        }

        overlayNode->setRect(0, dest_y, item->width(), dest_h);

        double tex_w = img_ptr->width();
        double tex_h = img_ptr->height();
        double final_src_x = src_x * dpr;
        double final_src_y = src_y * dpr;
        double final_src_w = src_w * dpr;
        double final_src_h = src_h * dpr;

        if (final_src_y < 0.0) final_src_y = 0.0;
        if (final_src_y + final_src_h > tex_h) {
            if (final_src_h > tex_h) final_src_h = tex_h;
            if (final_src_y + final_src_h > tex_h) final_src_y = tex_h - final_src_h;
        }
        if (final_src_x < 0.0) final_src_x = 0.0;
        if (final_src_x + final_src_w > tex_w) {
            if (final_src_w > tex_w) final_src_w = tex_w;
            if (final_src_x + final_src_w > tex_w) final_src_x = tex_w - final_src_w;
        }

        overlayNode->setSourceRect(final_src_x, final_src_y, final_src_w, final_src_h);
        overlayNode->setTexture(item->window()->createTextureFromImage(*img_ptr));
        overlayNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    }

    void sujian_update_cursor_rect(QSGTransformNode *root, QQuickItem *item,
        double cx, double cy, double cw, double ch, bool visible, unsigned int color_rgba) {
        if (!root) return;
        QSGRectangleNode *cursorNode = nullptr;
        // Cursor is always child[2] in three-layer layout
        if (root->childCount() > 2) {
            cursorNode = dynamic_cast<QSGRectangleNode*>(child_at(root,2));
        }
        if (!cursorNode) {
            cursorNode = item->window()->createRectangleNode();
            root->appendChildNode(cursorNode);
        }
        if (!visible) {
            cursorNode->setRect(QRectF(-100000, -100000, 0, 0));
            cursorNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
            return;
        }
        cursorNode->setRect(QRectF(cx, cy, cw, ch));
        cursorNode->setColor(QColor::fromRgba(color_rgba));
        cursorNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    }
}}

pub fn ensure_three_layer_nodes(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        ensure_three_layer_nodes(
            static_cast<QSGTransformNode*>(root_raw), item_ptr
        );
    })
}

/// SUJIAN_EDITOR_STATIC_ONLY mode: only ensure a single QSGImageNode
/// for the static text texture. No overlay or cursor nodes are created.
pub fn ensure_single_image_node(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*"
    ] {
        ensure_single_image_node(
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
        QSGImageNode *imgNode = nullptr;
        if (root->childCount() > 0) {
            imgNode = static_cast<QSGImageNode*>(root->firstChild());
        }
        if (!imgNode) {
            imgNode = item_ptr->window()->createImageNode();
            imgNode->setFiltering(QSGTexture::Nearest);
            imgNode->setOwnsTexture(true);
            root->appendChildNode(imgNode);
        }
        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);

        double tex_w = img_ptr->width();
        double tex_h = img_ptr->height();
        double final_src_x = src_x * dpr;
        double final_src_y = src_y * dpr;
        double final_src_w = src_w * dpr;
        double final_src_h = src_h * dpr;

        if (final_src_y < 0.0) final_src_y = 0.0;
        if (final_src_y + final_src_h > tex_h) {
            if (final_src_h > tex_h) {
                final_src_h = tex_h;
            }
            if (final_src_y + final_src_h > tex_h) {
                final_src_y = tex_h - final_src_h;
            }
        }
        if (final_src_x < 0.0) final_src_x = 0.0;
        if (final_src_x + final_src_w > tex_w) {
            if (final_src_w > tex_w) {
                final_src_w = tex_w;
            }
            if (final_src_x + final_src_w > tex_w) {
                final_src_x = tex_w - final_src_w;
            }
        }

        imgNode->setSourceRect(final_src_x, final_src_y, final_src_w, final_src_h);
        imgNode->setTexture(item_ptr->window()->createTextureFromImage(*img_ptr));
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
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
        auto *imgNode = static_cast<QSGImageNode*>(root->firstChild());
        if (!imgNode) return;
        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);

        double final_src_x = src_x * dpr;
        double final_src_y = src_y * dpr;
        double final_src_w = src_w * dpr;
        double final_src_h = src_h * dpr;

        if (imgNode->texture()) {
            QSize texSize = imgNode->texture()->textureSize();
            double tex_w = texSize.width();
            double tex_h = texSize.height();

            if (final_src_y < 0.0) final_src_y = 0.0;
            if (final_src_y + final_src_h > tex_h) {
                if (final_src_h > tex_h) {
                    final_src_h = tex_h;
                }
                if (final_src_y + final_src_h > tex_h) {
                    final_src_y = tex_h - final_src_h;
                }
            }
            if (final_src_x < 0.0) final_src_x = 0.0;
            if (final_src_x + final_src_w > tex_w) {
                if (final_src_w > tex_w) {
                    final_src_w = tex_w;
                }
                if (final_src_x + final_src_w > tex_w) {
                    final_src_x = tex_w - final_src_w;
                }
            }
        }

        imgNode->setSourceRect(final_src_x, final_src_y, final_src_w, final_src_h);
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    })
}

pub fn update_animation_overlay(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    image: Option<&qmetaobject::QImage>,
    scroll_y: f64,
    buffer_h: f64,
    dpr: f64,
) {
    let img_ptr = image.map(|i| i as *const qmetaobject::QImage).unwrap_or(std::ptr::null());
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        img_ptr as "const QImage*",
        scroll_y as "double", buffer_h as "double", dpr as "double"
    ] {
        sujian_update_animation_overlay(
            static_cast<QSGTransformNode*>(root_raw), item_ptr,
            img_ptr,
            0.0, scroll_y,
            img_ptr ? (double)img_ptr->width() / dpr : 0.0,
            buffer_h,
            0.0, buffer_h, dpr
        );
    })
}

pub fn update_cursor_rect(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    cx: f64,
    cy: f64,
    cw: f64,
    ch: f64,
    visible: bool,
    color_rgba: u32,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        cx as "double", cy as "double",
        cw as "double", ch as "double",
        visible as "bool",
        color_rgba as "unsigned int"
    ] {
        sujian_update_cursor_rect(
            static_cast<QSGTransformNode*>(root_raw), item_ptr,
            cx, cy, cw, ch, visible, color_rgba
        );
    })
}
