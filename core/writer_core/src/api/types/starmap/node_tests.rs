use super::*;
use crate::starmap::types::StarMapNodeKind;

#[test]
fn test_starmap_node_kind_dto_roundtrip() {
    for k in [
        StarMapNodeKind::Character,
        StarMapNodeKind::Event,
        StarMapNodeKind::Location,
        StarMapNodeKind::Item,
        StarMapNodeKind::Concept,
        StarMapNodeKind::Theme,
        StarMapNodeKind::Note,
        StarMapNodeKind::Organization,
        StarMapNodeKind::Timeline,
        StarMapNodeKind::Plot,
        StarMapNodeKind::Foreshadowing,
        StarMapNodeKind::Chapter,
        StarMapNodeKind::Custom,
    ] {
        let dto: StarMapNodeKindDto = k.clone().into();
        let back: StarMapNodeKind = dto.into();
        assert_eq!(back, k);
    }
}
