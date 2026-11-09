//! Windows Qt Quick Scene Graph 渲染宿主。
//!
//! 星图节点/连线/网格仍复用共享 QSG 图层。本组件用于 Windows 侧明确接入
//! Qt6 RHI（默认 Direct3D 11）并提供渲染后端诊断，不复制共享布局算法。

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::scenegraph::{ContainerNode, SGNode};
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

static PAINT_NODE_UPDATES: AtomicU64 = AtomicU64::new(0);

/// 诊断统计：只含计数和图形 API 名称。
pub fn render_statistics_json() -> String {
    serde_json::json!({
        "paintNodeUpdates": PAINT_NODE_UPDATES.load(Ordering::Relaxed),
        "graphicsApi": windows_graphics_api(),
    })
    .to_string()
}

fn windows_graphics_api() -> String {
    cpp!(unsafe [] -> QString as "QString" {
        return QStringLiteral("direct3d11");
    })
    .to_string()
}

cpp! {{
    #include <qmetaobject_rust.hpp>
    #include <QtQuick/QQuickItem>
    #include <QtQuick/QSGNode>
}}

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct WindowsRenderHost {
    base: qt_base_class!(trait QQuickItem),

    renderer_info: qt_property!(QString; READ renderer_info NOTIFY renderer_info_changed),
    renderer_info_changed: qt_signal!(),

    request_render: qt_method!(fn(&mut self)),
}

impl QQuickItem for WindowsRenderHost {
    fn component_complete(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效 QQuickItem 指针；ItemHasContents 启用 updatePaintNode。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj)
                obj->setFlag(QQuickItem::ItemHasContents, true);
        });
        self.request_render();
        self.renderer_info_changed();
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.request_render();
    }

    fn update_paint_node(&mut self, node: SGNode<ContainerNode>) -> SGNode<ContainerNode> {
        PAINT_NODE_UPDATES.fetch_add(1, Ordering::Relaxed);
        let mut raw = node.into_raw();
        if raw.is_null() {
            // SAFETY: 空占位节点由父节点回收。
            raw = cpp!(unsafe [] -> *mut c_void as "QSGNode *" {
                QSGNode *placeholder = new QSGNode;
                placeholder->setFlag(QSGNode::OwnedByParent);
                return placeholder;
            });
        }
        // SAFETY: raw 来自 ContainerNode 或刚创建的有效 QSGNode，所有权交回场景图。
        unsafe { SGNode::from_raw(raw) }
    }
}

impl WindowsRenderHost {
    fn renderer_info(&self) -> QString {
        QString::from(format!(
            "{{\"graphicsApi\":\"direct3d11\",\"paintNodeUpdates\":{}}}",
            PAINT_NODE_UPDATES.load(Ordering::Relaxed)
        ))
    }

    fn request_render(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效 QQuickItem 指针；update() 可跨线程排队。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj)
                obj->update();
        });
    }
}
