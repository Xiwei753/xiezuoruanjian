//! `WriterCore::resolve_starmap_path` 的行为测试。
//!
//! 验证 root starmap + 路径段逐段解析：空路径落在 root，逐段 EnterEmbed 落在
//! 各层子图；路径失效（Embed 被删/不存在）时返回错误，不静默落回裸目标 ID。

use crate::facade::WriterCore;
use crate::starmap::semantic::{StarMapProvenance, StarMapTargetDetail};
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::{StarMapEmbed, StarMapPoint};
use tempfile::tempdir;

fn new_core(temp: &std::path::Path) -> WriterCore {
    let projects_root = temp.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    WriterCore::new(temp, &projects_root)
}

fn create_starmap(core: &WriterCore, title: &str) -> String {
    core.create_starmap(title, "", None).unwrap().starmap_id
}

fn embed_child(core: &WriterCore, host: &str, target: &str, instance_id: &str) {
    let embed = StarMapEmbed {
        instance_id: instance_id.to_string(),
        target_starmap_id: target.to_string(),
        label: None,
        position: StarMapPoint { x: 0.0, y: 0.0 },
        host_path: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    core.add_starmap_embed(host, embed).unwrap();
}

/// root + 路径段逐段解析：空路径落在 root 本身，逐段 EnterEmbed 落在各层子图。
#[test]
fn resolve_starmap_path_walks_embed_segments() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    let grandchild = create_starmap(&core, "孙图");
    embed_child(&core, &root, &child, "em_root_child");
    embed_child(&core, &child, &grandchild, "em_child_grand");

    let empty = core.resolve_starmap_path(&root, vec![]).unwrap();
    assert_eq!(empty.final_starmap_id, root);

    let one = core
        .resolve_starmap_path(
            &root,
            vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
        )
        .unwrap();
    assert_eq!(one.final_starmap_id, child);

    let two = core.resolve_starmap_path(
        &root,
        vec![
            StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            },
            StarMapPathSegment::EnterPortal {
                node_id: "n_missing".to_string(),
            },
        ],
    );
    assert!(two.is_err(), "不存在的 portal 段必须解析失败");

    let two_embeds = core
        .resolve_starmap_path(
            &root,
            vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "em_root_child".to_string(),
                },
                StarMapPathSegment::EnterEmbed {
                    instance_id: "em_child_grand".to_string(),
                },
            ],
        )
        .unwrap();
    assert_eq!(two_embeds.final_starmap_id, grandchild);
}

/// 路径失效（Embed 被删/不存在）时返回错误，而不是静默落回裸目标 ID。
#[test]
fn resolve_starmap_path_rejects_missing_embed() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    embed_child(&core, &root, &child, "em_root_child");

    let missing = core.resolve_starmap_path(
        &root,
        vec![StarMapPathSegment::EnterEmbed {
            instance_id: "em_removed".to_string(),
        }],
    );
    assert!(missing.is_err(), "已删除的 Embed 段必须解析失败");

    let resolved = core
        .resolve_starmap_path(
            &root,
            vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
        )
        .unwrap();
    assert_eq!(resolved.final_starmap_id, child);
}
