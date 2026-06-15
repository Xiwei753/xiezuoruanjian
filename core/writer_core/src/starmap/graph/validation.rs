use crate::error::{Error, Result};
use crate::starmap::types::*;

pub(crate) fn validate_graph(workspace: &std::path::Path, graph: &StarMapGraph) -> Result<()> {
    let node_ids = validate_nodes(workspace, graph)?;
    validate_edges(workspace, graph, &node_ids)?;
    validate_embeds(workspace, graph, &node_ids)?;
    validate_links(workspace, graph, &node_ids)?;
    Ok(())
}

fn validate_nodes(
    workspace: &std::path::Path,
    graph: &StarMapGraph,
) -> Result<std::collections::HashSet<String>> {
    let mut node_ids = std::collections::HashSet::new();
    for node in &graph.nodes {
        if !node_ids.insert(node.id.clone()) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate node ID",
            )));
        }

        if let crate::starmap::semantic::StarMapNodeContent::ChapterRef {
            range_start,
            range_end,
            ..
        } = &node.content
        {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Content range_start cannot be greater than range_end",
                    )));
                }
            }
        }

        let mut anchor_ids = std::collections::HashSet::new();
        for anchor in &node.anchors {
            if !anchor_ids.insert(&anchor.anchor_id) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Duplicate anchor ID in node",
                )));
            }
            if let crate::starmap::semantic::StarMapAnchorTarget::ChapterRange {
                range_start,
                range_end,
                ..
            } = &anchor.target
            {
                if let (Some(s), Some(e)) = (range_start, range_end) {
                    if s > e {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Anchor range_start cannot be greater than range_end",
                        )));
                    }
                }
            }
        }

        if let Some(portal) = &node.portal {
            if portal.mode == crate::starmap::semantic::StarMapPortalMode::EnterChild {
                let target_id = portal
                    .deep_target
                    .as_ref()
                    .map(|t| t.starmap_id.clone())
                    .unwrap_or_else(|| portal.target_starmap_id.clone());

                if crate::starmap::load_starmap_meta(workspace, &target_id).is_err() {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Portal target starmap does not exist",
                    )));
                }

                if let Some(dt) = &portal.deep_target {
                    let status = super::resolve::resolve_deep_target(workspace, dt);
                    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                    match status {
                        CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
                        | InvalidRange => {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Deep target resolve failed: {:?}", status),
                            )));
                        }
                        _ => {}
                    }
                }
            }
        }

        crate::starmap::semantic::validate_display_policy(&node.display_policy)?;
    }
    Ok(node_ids)
}

fn validate_edges(
    workspace: &std::path::Path,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    for edge in &graph.edges {
        let validate_edge_endpoint = |ep: &Option<StarMapEdgeEndpoint>,
                                      legacy_id: &Option<String>,
                                      legacy_target: &Option<
            crate::starmap::semantic::StarMapDeepTarget,
        >,
                                      endpoint_name: &str|
         -> Result<()> {
            if let Some(endpoint) = ep {
                match endpoint {
                    StarMapEdgeEndpoint::Node { node_id } => {
                        if !node_ids.contains(node_id) {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "Edge {} endpoint references non-existent node",
                                    endpoint_name
                                ),
                            )));
                        }
                    }
                    StarMapEdgeEndpoint::Anchor { node_id, anchor_id } => {
                        let mut anchor_found = false;
                        if let Some(node) = graph.nodes.iter().find(|n| &n.id == node_id) {
                            if node.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                                anchor_found = true;
                            }
                        }
                        if !anchor_found {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "Edge {} endpoint references non-existent anchor",
                                    endpoint_name
                                ),
                            )));
                        }
                    }
                    StarMapEdgeEndpoint::Starmap => {}
                    StarMapEdgeEndpoint::DeepTarget { target } => {
                        let status = super::resolve::resolve_deep_target(workspace, target);
                        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                        match status {
                            CycleDetected | TooDeep | MissingStarmap | MissingNode
                            | MissingAnchor | InvalidRange => {
                                return Err(Error::Io(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("Edge deep target resolve failed: {:?}", status),
                                )));
                            }
                            _ => {}
                        }
                    }
                }
            } else if let Some(target) = legacy_target {
                let status = super::resolve::resolve_deep_target(workspace, target);
                use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                match status {
                    CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
                    | InvalidRange => {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Edge deep target resolve failed: {:?}", status),
                        )));
                    }
                    _ => {}
                }
            } else if let Some(id) = legacy_id {
                if !node_ids.contains(id) {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Edge {} references non-existent node", endpoint_name),
                    )));
                }
            }
            Ok(())
        };

        validate_edge_endpoint(&edge.from_endpoint, &edge.from, &edge.from_target, "from")?;
        validate_edge_endpoint(&edge.to_endpoint, &edge.to, &edge.to_target, "to")?;
    }
    Ok(())
}

fn validate_embeds(
    workspace: &std::path::Path,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut instance_ids = std::collections::HashSet::new();
    for embed in &graph.embeds {
        if !instance_ids.insert(&embed.instance_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate embed instance_id",
            )));
        }
        if embed.target_starmap_id == graph.starmap_id {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Self-embed is prohibited",
            )));
        }

        if crate::starmap::load_starmap_meta(workspace, &embed.target_starmap_id).is_err() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Embed target starmap does not exist",
            )));
        }

        let p = &embed.placement;
        if p.width < 0.0
            || p.height < 0.0
            || p.scale <= 0.0
            || p.width.is_nan()
            || p.height.is_nan()
            || p.scale.is_nan()
            || p.x.is_nan()
            || p.y.is_nan()
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid embed placement values",
            )));
        }

        let tvp = &embed.target_viewport;
        if tvp.scale <= 0.0 || tvp.scale.is_nan() || tvp.offset_x.is_nan() || tvp.offset_y.is_nan()
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid embed target_viewport values",
            )));
        }

        if let Some(sni) = &embed.source_node_id {
            if !node_ids.contains(sni) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Embed source_node_id does not exist",
                )));
            }
        }

        if let Some(ep) = &embed.host_endpoint {
            match ep {
                StarMapEndpoint::Node { node_id } => {
                    if !node_ids.contains(node_id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Embed host_endpoint references non-existent node",
                        )));
                    }
                }
                StarMapEndpoint::Anchor { node_id, anchor_id } => {
                    let mut anchor_found = false;
                    if let Some(node) = graph.nodes.iter().find(|n| &n.id == node_id) {
                        if node.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                            anchor_found = true;
                        }
                    }
                    if !anchor_found {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Embed host_endpoint references non-existent anchor",
                        )));
                    }
                }
                StarMapEndpoint::Starmap => {}
            }
        }

        crate::starmap::semantic::validate_display_policy(&embed.display_policy)?;
    }
    Ok(())
}

fn validate_links(
    workspace: &std::path::Path,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut link_ids = std::collections::HashSet::new();
    for link in &graph.links {
        if !link_ids.insert(&link.link_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate link_id",
            )));
        }
        match &link.source {
            StarMapEndpoint::Node { node_id } => {
                if !node_ids.contains(node_id) {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Link source node does not exist",
                    )));
                }
            }
            StarMapEndpoint::Anchor { node_id, anchor_id } => {
                if let Some(n) = graph.nodes.iter().find(|n| &n.id == node_id) {
                    if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Link source anchor does not exist",
                        )));
                    }
                } else {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Link source node for anchor does not exist",
                    )));
                }
            }
            StarMapEndpoint::Starmap => {}
        }

        let status = super::resolve::resolve_deep_target(workspace, &link.target);
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        match status {
            CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
            | InvalidRange => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Link deep target resolve failed: {:?}", status),
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn validate_layout(layout: &StarMapLayout) -> Result<()> {
    for node in &layout.nodes {
        if node.scale <= 0.0 || node.scale.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout scale must be > 0",
            )));
        }
        if node.depth.is_nan() || node.focus_weight.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout depth or focus_weight cannot be NaN",
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_viewport(viewport: &StarMapViewport) -> Result<()> {
    if viewport.scale <= 0.0 || !viewport.scale.is_finite() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Viewport scale must be a finite value > 0",
        )));
    }
    if !viewport.offset_x.is_finite()
        || !viewport.offset_y.is_finite()
        || !viewport.width.is_finite()
        || !viewport.height.is_finite()
    {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Viewport values must be finite",
        )));
    }
    Ok(())
}