use crate::error::{Error, Result};
use crate::starmap::now_epoch;
use crate::starmap::types::*;

pub(crate) fn add_starmap_embed(
    workspace: &std::path::Path,
    starmap_id: &str,
    embed: StarMapEmbed,
) -> Result<StarMapEmbed> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    if graph
        .embeds
        .iter()
        .any(|e| e.instance_id == embed.instance_id)
    {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Duplicate embed instance_id",
        )));
    }
    graph.embeds.push(embed.clone());
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(embed)
}

pub(crate) fn update_starmap_embed(
    workspace: &std::path::Path,
    starmap_id: &str,
    instance_id: &str,
    patch: StarMapEmbedPatch,
) -> Result<StarMapEmbed> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    if let Some(embed) = graph
        .embeds
        .iter_mut()
        .find(|e| e.instance_id == instance_id)
    {
        if let Some(l) = patch.label {
            embed.label = l;
        }
        if let Some(dp) = patch.display_policy {
            embed.display_policy = dp;
        }
        if let Some(ob) = patch.open_behavior {
            embed.open_behavior = ob;
        }
        if let Some(Some(pl)) = patch.placement {
            embed.placement = pl;
        }
        if let Some(Some(vp)) = patch.target_viewport {
            embed.target_viewport = vp;
        }
        if let Some(Some(vp)) = patch.viewport {
            embed.placement.width = vp.width;
            embed.placement.height = vp.height;
            embed.target_viewport.scale = vp.scale;
            embed.target_viewport.offset_x = vp.offset_x;
            embed.target_viewport.offset_y = vp.offset_y;
        }
        if let Some(sni) = patch.source_node_id {
            embed.source_node_id = sni;
        }
        if let Some(ep) = patch.host_endpoint {
            embed.host_endpoint = ep;
        }
        if let Some(Some(anchor_id)) = patch.host_anchor {
            if let Some(node_id) = &embed.source_node_id {
                embed.host_endpoint = Some(StarMapEndpoint::Anchor {
                    node_id: node_id.clone(),
                    anchor_id,
                });
            }
        }
        embed.updated_at = now_epoch();
        let updated = embed.clone();
        graph.updated_at = now_epoch();
        super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Embed not found",
        )))
    }
}

pub(crate) fn delete_starmap_embed(
    workspace: &std::path::Path,
    starmap_id: &str,
    instance_id: &str,
) -> Result<()> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.embeds.len();
    graph.embeds.retain(|e| e.instance_id != instance_id);
    if graph.embeds.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Embed not found",
        )));
    }
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}
