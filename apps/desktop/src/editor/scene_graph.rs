use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGImageNode>
    #include <QDebug>

    // Qt 6.11 compatible helpers — QSGNode no longer has childAt() or
    // insertChildNode(node, index).  Use childAtIndex() and
    // insertChildNodeBefore() / appendChildNode() instead.

    static QSGNode *child_at(QSGNode *root, int index) {
        return root && index >= 0 && index < root->childCount()
            ? root->childAtIndex(index)
            : nullptr;
    }

    // Single-layer scene graph layout:
    //   child[0] = QSGImageNode  — static text texture
    // (Animation overlay is handled by QML EditorAnimationOverlay.
    //  Cursor is handled by QML SmoothCursor.)

    void ensure_single_image_node(QSGTransformNode *root, QQuickItem *item) {
        if (!root || !item) return;

        // Remove any extra children beyond child[0] (overlay, cursor, etc.)
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
}}

/// Default scene graph layout: only ensure a single QSGImageNode
/// for the static text texture. No overlay or cursor nodes are created.
/// Animation overlay is handled by QML EditorAnimationOverlay.
/// Cursor is handled by QML SmoothCursor.
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
        // destRect uses QML logical coordinates
        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);

        // sourceRect uses physical pixel coordinates (multiply logical by dpr).
        // The QImage has DPR=1.0, so textureSize == image pixel size.
        double phys_src_x = src_x * dpr;
        double phys_src_y = src_y * dpr;
        double phys_src_w = src_w * dpr;
        double phys_src_h = src_h * dpr;

        // Clamp source rect to image bounds (physical pixel coords)
        double phys_img_w = static_cast<double>(img_ptr->width());
        double phys_img_h = static_cast<double>(img_ptr->height());

        if (phys_src_y < 0.0) phys_src_y = 0.0;
        if (phys_src_y + phys_src_h > phys_img_h) {
            if (phys_src_h > phys_img_h) {
                phys_src_h = phys_img_h;
            }
            if (phys_src_y + phys_src_h > phys_img_h) {
                phys_src_y = phys_img_h - phys_src_h;
            }
        }
        if (phys_src_x < 0.0) phys_src_x = 0.0;
        if (phys_src_x + phys_src_w > phys_img_w) {
            if (phys_src_w > phys_img_w) {
                phys_src_w = phys_img_w;
            }
            if (phys_src_x + phys_src_w > phys_img_w) {
                phys_src_x = phys_img_w - phys_src_w;
            }
        }

        imgNode->setSourceRect(phys_src_x, phys_src_y, phys_src_w, phys_src_h);
        QSGTexture *tex = item_ptr->window()->createTextureFromImage(*img_ptr);
        tex->setFiltering(QSGTexture::Nearest);
        imgNode->setTexture(tex);
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);

        static bool logged_once = false;
        if (!logged_once) {
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
        auto *imgNode = static_cast<QSGImageNode*>(root->firstChild());
        if (!imgNode) return;
        // destRect uses QML logical coordinates
        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);

        // sourceRect uses physical pixel coordinates (multiply logical by dpr)
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
                if (phys_src_h > phys_img_h) {
                    phys_src_h = phys_img_h;
                }
                if (phys_src_y + phys_src_h > phys_img_h) {
                    phys_src_y = phys_img_h - phys_src_h;
                }
            }
            if (phys_src_x < 0.0) phys_src_x = 0.0;
            if (phys_src_x + phys_src_w > phys_img_w) {
                if (phys_src_w > phys_img_w) {
                    phys_src_w = phys_img_w;
                }
                if (phys_src_x + phys_src_w > phys_img_w) {
                    phys_src_x = phys_img_w - phys_src_w;
                }
            }
        }

        imgNode->setSourceRect(phys_src_x, phys_src_y, phys_src_w, phys_src_h);
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    })
}
