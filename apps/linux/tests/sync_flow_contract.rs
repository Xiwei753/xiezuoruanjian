// =============================================================================
// sync_flow_contract.rs — 同步流隔离契约测试
// =============================================================================
//
// 职责：验证 SyncBackend 和 SyncPage.qml 之间关于 operation_id 异步隔离的设计契约
// 引用：测试 src/backend/sync_backend.rs 和 qml/SyncPage.qml
// =============================================================================

use std::fs;

#[test]
fn test_sync_backend_exposes_operation_id_methods() {
    let sync_backend = fs::read_to_string("src/backend/sync_backend.rs").expect("read sync backend");
    
    // 验证同步、诊断、预运行方法都返回 QString 作为唯一的 operation_id
    assert!(
        sync_backend.contains("perform_sync: qt_method!(fn(&mut self) -> QString)"),
        "SyncBackend must return operation_id (QString) from perform_sync"
    );
    assert!(
        sync_backend.contains("perform_sync_diagnostics: qt_method!(fn(&mut self) -> QString)"),
        "SyncBackend must return operation_id (QString) from perform_sync_diagnostics"
    );
    assert!(
        sync_backend.contains("perform_sync_dry_run: qt_method!(fn(&mut self) -> QString)"),
        "SyncBackend must return operation_id (QString) from perform_sync_dry_run"
    );
}

#[test]
fn test_sync_page_uses_operation_id_check() {
    let sync_page = fs::read_to_string("qml/SyncPage.qml").expect("read SyncPage.qml");
    
    // 验证 SyncPage 声明了用于缓存当前操作 ID 的 local property
    assert!(
        sync_page.contains("property string activeOperationId:"),
        "SyncPage must declare activeOperationId"
    );
    
    // 验证 SyncPage 在 Connections 接收结果时校验了 obj.operation_id
    assert!(
        sync_page.contains("obj.operation_id === root.activeOperationId"),
        "SyncPage must verify operation_id matches activeOperationId before updating text"
    );
    
    // 验证 SyncPage 按钮在 clicked 时记录了 opId
    assert!(
        sync_page.contains("root.activeOperationId = opId"),
        "SyncPage buttons must store the returned operation ID to activeOperationId"
    );
}
