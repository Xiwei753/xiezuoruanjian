mod crud;
mod load;
mod migration;
mod save;
mod snapshot;

use crate::starmap::package_storage;
use crate::starmap::types::*;

fn write_to_bucket(dir: &std::path::Path, subdir: &str, id: &str, json: &str) {
    let bucket = package_storage::bucket_for_id(id);
    let path = dir.join(subdir).join(bucket).join(format!("{}.json", id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, json).unwrap();
}
fn make_test_node(id: &str, title: &str) -> StarMapNode {
    use crate::starmap::semantic::{
        StarMapDisplayPolicy, StarMapNodeContent, StarMapOpenBehavior, StarMapProvenance,
    };
    StarMapNode {
        id: id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        tags: vec![],
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    }
}
fn make_test_link(link_id: &str, label: &str) -> StarMapLink {
    use crate::starmap::semantic::{StarMapDeepTarget, StarMapTargetDetail};
    StarMapLink {
        link_id: link_id.to_string(),
        source: StarMapEndpoint::Starmap,
        target: StarMapDeepTarget {
            starmap_id: "other".to_string(),
            path: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        label: Some(label.to_string()),
        created_at: 0,
        updated_at: 0,
    }
}
