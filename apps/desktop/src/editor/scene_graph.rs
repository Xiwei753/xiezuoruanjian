use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGImageNode>
    #include <QtQuick/QSGRectangleNode>

    void sujian_clean_cursor_nodes(QSGNode *root) {
        if (!root) return;
        auto *transformNode = static_cast<QSGTransformNode*>(root);
        while (transformNode->childCount() > 1) {
            QSGNode *child = transformNode->lastChild();
            transformNode->removeChildNode(child);
            delete child;
        }
    }

    void sujian_update_cursor_rect(QSGTransformNode *root, QQuickItem *item,
        double cx, double cy, double cw, double ch, bool visible, unsigned int color_rgba) {
        if (!root) return;
        QSGRectangleNode *cursorNode = nullptr;
        if (root->childCount() > 1) {
            cursorNode = dynamic_cast<QSGRectangleNode*>(root->lastChild());
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
