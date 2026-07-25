use super::workspace::*;
use crate::workspace::RecentEdit;

#[test]
fn test_recent_edit_dto_conversion() {
    let edit = RecentEdit {
        project_id: "proj_123".to_string(),
        volume_id: "vol_456".to_string(),
        chapter_id: "chap_789".to_string(),
        timestamp: "2023-10-01T12:00:00Z".to_string(),
    };
    let dto: RecentEditDto = edit.clone().into();
    assert_eq!(dto.project_id, edit.project_id);
    assert_eq!(dto.volume_id, edit.volume_id);
    assert_eq!(dto.chapter_id, edit.chapter_id);
    assert_eq!(dto.timestamp, edit.timestamp);
}
