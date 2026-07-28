use crate::error::{Error, Result};
use crate::starmap::now_epoch;
use crate::starmap::types::*;

pub(crate) fn add_starmap_link(
    workspace: &std::path::Path,
    starmap_id: &str,
    link: StarMapLink,
) -> Result<StarMapLink> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    if graph.links.iter().any(|l| l.link_id == link.link_id) {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Duplicate link_id",
        )));
    }
    graph.links.push(link.clone());
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(link)
}

pub(crate) fn update_starmap_link(
    workspace: &std::path::Path,
    starmap_id: &str,
    link_id: &str,
    patch: StarMapLinkPatch,
) -> Result<StarMapLink> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    if let Some(link) = graph.links.iter_mut().find(|l| l.link_id == link_id) {
        if let Some(s) = patch.source {
            link.source = s;
        }
        if let Some(t) = patch.target {
            link.target = t;
        }
        if let Some(l) = patch.label {
            link.label = l;
        }
        link.updated_at = now_epoch();
        let updated = link.clone();
        graph.updated_at = now_epoch();
        super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Link not found",
        )))
    }
}

pub(crate) fn delete_starmap_link(
    workspace: &std::path::Path,
    starmap_id: &str,
    link_id: &str,
) -> Result<()> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.links.len();
    graph.links.retain(|l| l.link_id != link_id);
    if graph.links.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Link not found",
        )));
    }
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}
