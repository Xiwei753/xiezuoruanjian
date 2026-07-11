use std::fmt;

#[derive(Clone, Debug)]
pub(crate) struct ShapedGlyph {
    pub glyph_index: u32,
    pub glyph_position_x: f64,
    pub glyph_position_y: f64,
    pub string_index: usize,
    pub advance_width: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedCluster {
    pub string_start: usize,
    pub string_end: usize,
    pub glyph_start: usize,
    pub glyph_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RawFontCacheKey {
    pub family_name: String,
    pub style_name: String,
    pub weight: i32,
    pub pixel_size: i32,
}

impl RawFontCacheKey {
    pub fn new(family_name: &str, style_name: &str, weight: i32, pixel_size: i32) -> Self {
        Self {
            family_name: family_name.to_string(),
            style_name: style_name.to_string(),
            weight,
            pixel_size,
        }
    }

    pub fn as_stable_id(&self) -> String {
        if self.style_name.is_empty() {
            format!("{}:w{}:s{}", self.family_name, self.weight, self.pixel_size)
        } else {
            format!("{}:{}:w{}:s{}", self.family_name, self.style_name, self.weight, self.pixel_size)
        }
    }

    pub fn raw_font_family_parsed(&self) -> &str {
        &self.family_name
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct RunFlags: u8 {
        const RTL = 0x01;
        const UNDERLINE = 0x02;
        const OVERLINE = 0x04;
        const STRIKE_OUT = 0x08;
    }
}

#[derive(Clone)]
pub(crate) struct ShapedVisualRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub clusters: Vec<ShapedCluster>,
    pub raw_font_key: RawFontCacheKey,
    pub flags: RunFlags,
    pub source_string_start: usize,
    pub source_string_end: usize,
    pub baseline_y: f64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_w: f64,
    pub visual_h: f64,
    pub texture_atlas_x: f64,
    pub texture_atlas_y: f64,
    pub texture_atlas_w: f64,
    pub texture_atlas_h: f64,
    pub texture_translate_x: f64,
    pub texture_translate_y: f64,
    pub qglyphrun_index: i32,
    pub para_text: Option<String>,
    pub qtextline_idx: Option<i32>,
    pub paragraph_wrap_w: Option<f64>,
    pub para_indent: Option<f64>,
}

impl ShapedVisualRun {
    pub fn total_advance_width(&self) -> f64 {
        self.glyphs.iter().map(|g| g.advance_width).sum()
    }

    pub fn string_range(&self) -> (usize, usize) {
        (self.source_string_start, self.source_string_end)
    }

    pub fn is_rtl(&self) -> bool {
        self.flags.contains(RunFlags::RTL)
    }

    pub fn has_underline(&self) -> bool {
        self.flags.contains(RunFlags::UNDERLINE)
    }

    pub fn font_id(&self) -> String {
        self.raw_font_key.as_stable_id()
    }
}

impl fmt::Debug for ShapedVisualRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShapedVisualRun")
            .field("glyph_count", &self.glyphs.len())
            .field("cluster_count", &self.clusters.len())
            .field("font_id", &self.raw_font_key.as_stable_id())
            .field("string_range", &(self.source_string_start..self.source_string_end))
            .field("flags", &self.flags)
            .field("baseline_y", &self.baseline_y)
            .field("visual_bounds", &format!("({:.1},{:.1},{:.1},{:.1})", self.visual_x, self.visual_y, self.visual_w, self.visual_h))
            .finish()
    }
}

pub(crate) fn derive_clusters_from_glyphs(glyphs: &[ShapedGlyph]) -> Vec<ShapedCluster> {
    if glyphs.is_empty() {
        return Vec::new();
    }

    let mut clusters: Vec<ShapedCluster> = Vec::new();
    let mut cluster_string_start = glyphs[0].string_index;
    let mut cluster_string_end = glyphs[0].string_index + 1;
    let mut cluster_glyph_start = 0usize;

    for (i, glyph) in glyphs.iter().enumerate().skip(1) {
        if glyph.string_index == glyphs[i - 1].string_index {
            cluster_string_end = cluster_string_end.max(glyph.string_index + 1);
            continue;
        }

        clusters.push(ShapedCluster {
            string_start: cluster_string_start,
            string_end: cluster_string_end,
            glyph_start: cluster_glyph_start,
            glyph_end: i,
        });
        cluster_string_start = glyph.string_index;
        cluster_string_end = glyph.string_index + 1;
        cluster_glyph_start = i;
    }

    let last_idx = glyphs.len();
    clusters.push(ShapedCluster {
        string_start: cluster_string_start,
        string_end: cluster_string_end,
        glyph_start: cluster_glyph_start,
        glyph_end: last_idx,
    });

    clusters
}

#[derive(Clone, Debug)]
pub(crate) struct RunMapping {
    pub old_run_index: usize,
    pub new_run_index: usize,
    pub same_shaping: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ReflowVisualSnapshot {
    pub old_shaped_runs: Vec<ShapedVisualRun>,
    pub new_shaped_runs: Vec<ShapedVisualRun>,
    pub old_positions: Vec<(f64, f64, f64)>,
    pub new_positions: Vec<(f64, f64, f64)>,
    pub old_baselines: Vec<f64>,
    pub new_baselines: Vec<f64>,
    pub old_bounds: Vec<(f64, f64, f64, f64)>,
    pub new_bounds: Vec<(f64, f64, f64, f64)>,
    pub run_mapping: Vec<RunMapping>,
    pub unchanged_run_identity: Vec<bool>,
    pub changed_shaping: Vec<bool>,
}

impl ReflowVisualSnapshot {
    pub fn new(
        old_shaped_runs: Vec<ShapedVisualRun>,
        new_shaped_runs: Vec<ShapedVisualRun>,
    ) -> Self {
        let old_positions: Vec<(f64, f64, f64)> = old_shaped_runs
            .iter()
            .map(|r| (r.visual_x, r.visual_y, r.baseline_y))
            .collect();
        let new_positions: Vec<(f64, f64, f64)> = new_shaped_runs
            .iter()
            .map(|r| (r.visual_x, r.visual_y, r.baseline_y))
            .collect();

        let old_baselines: Vec<f64> = old_shaped_runs.iter().map(|r| r.baseline_y).collect();
        let new_baselines: Vec<f64> = new_shaped_runs.iter().map(|r| r.baseline_y).collect();

        let old_bounds: Vec<(f64, f64, f64, f64)> = old_shaped_runs
            .iter()
            .map(|r| (r.visual_x, r.visual_y, r.visual_w, r.visual_h))
            .collect();
        let new_bounds: Vec<(f64, f64, f64, f64)> = new_shaped_runs
            .iter()
            .map(|r| (r.visual_x, r.visual_y, r.visual_w, r.visual_h))
            .collect();

        // Build explicit old→new run mapping based on string range overlap,
        // not just array index alignment.
        let mut run_mapping: Vec<RunMapping> = Vec::new();
        for (old_idx, old_run) in old_shaped_runs.iter().enumerate() {
            let old_range = old_run.string_range();
            let mut best_new_idx: Option<usize> = None;
            let mut best_overlap: usize = 0;
            for (new_idx, new_run) in new_shaped_runs.iter().enumerate() {
                let new_range = new_run.string_range();
                let overlap_start = old_range.0.max(new_range.0);
                let overlap_end = old_range.1.min(new_range.1);
                if overlap_end > overlap_start {
                    let overlap = overlap_end - overlap_start;
                    if overlap > best_overlap {
                        best_overlap = overlap;
                        best_new_idx = Some(new_idx);
                    }
                }
            }
            if let Some(ni) = best_new_idx {
                let new_run = &new_shaped_runs[ni];
                let same_font = old_run.raw_font_key == new_run.raw_font_key;
                let same_glyphs = old_run.glyphs.len() == new_run.glyphs.len()
                    && old_run
                        .glyphs
                        .iter()
                        .zip(new_run.glyphs.iter())
                        .all(|(og, ng)| og.glyph_index == ng.glyph_index);
                let same_string_range =
                    old_run.source_string_start == new_run.source_string_start
                        && old_run.source_string_end == new_run.source_string_end;
                run_mapping.push(RunMapping {
                    old_run_index: old_idx,
                    new_run_index: ni,
                    same_shaping: same_font && same_glyphs && same_string_range,
                });
            }
        }

        let max_count = old_shaped_runs.len().max(new_shaped_runs.len());
        let mut unchanged_run_identity = vec![false; max_count];
        let mut changed_shaping = vec![true; max_count];

        // Populate per-new-run identity flags from the mapping
        for mapping in &run_mapping {
            if mapping.same_shaping {
                unchanged_run_identity[mapping.new_run_index] = true;
                changed_shaping[mapping.new_run_index] = false;
            }
        }

        ReflowVisualSnapshot {
            old_shaped_runs,
            new_shaped_runs,
            old_positions,
            new_positions,
            old_baselines,
            new_baselines,
            old_bounds,
            new_bounds,
            run_mapping,
            unchanged_run_identity,
            changed_shaping,
        }
    }

    pub fn can_reuse_texture_for_run(&self, run_index: usize) -> bool {
        self.unchanged_run_identity
            .get(run_index)
            .copied()
            .unwrap_or(false)
    }

    pub fn run_needs_crossfade(&self, run_index: usize) -> bool {
        self.changed_shaping
            .get(run_index)
            .copied()
            .unwrap_or(true)
    }

    pub fn old_run_for_new(&self, new_run_index: usize) -> Option<&ShapedVisualRun> {
        self.run_mapping
            .iter()
            .find(|m| m.new_run_index == new_run_index)
            .and_then(|m| self.old_shaped_runs.get(m.old_run_index))
    }

    pub fn old_position_for_new(&self, new_run_index: usize) -> Option<(f64, f64, f64)> {
        self.run_mapping
            .iter()
            .find(|m| m.new_run_index == new_run_index)
            .and_then(|m| self.old_positions.get(m.old_run_index))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_glyph(index: u32, string_index: usize, x: f64, w: f64) -> ShapedGlyph {
        ShapedGlyph {
            glyph_index: index,
            glyph_position_x: x,
            glyph_position_y: 0.0,
            string_index,
            advance_width: w,
        }
    }

    #[test]
    fn test_derive_clusters_simple() {
        let glyphs = vec![
            make_glyph(1, 0, 0.0, 10.0),
            make_glyph(2, 1, 10.0, 10.0),
            make_glyph(3, 2, 20.0, 10.0),
        ];
        let clusters = derive_clusters_from_glyphs(&glyphs);
        assert_eq!(clusters.len(), 3);
        assert_eq!(clusters[0].string_start, 0);
        assert_eq!(clusters[0].string_end, 1);
        assert_eq!(clusters[1].string_start, 1);
        assert_eq!(clusters[1].string_end, 2);
    }

    #[test]
    fn test_derive_clusters_ligature() {
        let glyphs = vec![
            make_glyph(10, 0, 0.0, 15.0),
            make_glyph(11, 0, 0.0, 0.0),
            make_glyph(12, 1, 15.0, 10.0),
        ];
        let clusters = derive_clusters_from_glyphs(&glyphs);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].string_start, 0);
        assert_eq!(clusters[0].string_end, 1);
        assert_eq!(clusters[0].glyph_start, 0);
        assert_eq!(clusters[0].glyph_end, 2);
        assert_eq!(clusters[1].string_start, 1);
        assert_eq!(clusters[1].string_end, 2);
        assert_eq!(clusters[1].glyph_start, 2);
        assert_eq!(clusters[1].glyph_end, 3);
    }

    #[test]
    fn test_derive_clusters_multi_char_glyph() {
        let glyphs = vec![
            make_glyph(20, 0, 0.0, 20.0),
            make_glyph(21, 2, 20.0, 10.0),
        ];
        let clusters = derive_clusters_from_glyphs(&glyphs);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].string_start, 0);
        assert_eq!(clusters[0].string_end, 1);
        assert_eq!(clusters[1].string_start, 2);
        assert_eq!(clusters[1].string_end, 3);
    }

    #[test]
    fn test_derive_clusters_emoji() {
        let glyphs = vec![
            make_glyph(30, 0, 0.0, 16.0),
            make_glyph(31, 0, 0.0, 0.0),
            make_glyph(32, 2, 16.0, 10.0),
        ];
        let clusters = derive_clusters_from_glyphs(&glyphs);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].glyph_count(), 2);
        assert_eq!(clusters[1].glyph_count(), 1);
    }

    #[test]
    fn test_derive_clusters_empty() {
        let clusters = derive_clusters_from_glyphs(&[]);
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_raw_font_cache_key_stable_id() {
        let key = RawFontCacheKey::new("Noto Sans", "Regular", 50, 16);
        let id = key.as_stable_id();
        assert!(id.contains("Noto Sans"));
        assert!(id.contains("w50"));
        assert!(id.contains("s16"));
    }

    #[test]
    fn test_raw_font_cache_key_equality() {
        let k1 = RawFontCacheKey::new("Arial", "Bold", 75, 14);
        let k2 = RawFontCacheKey::new("Arial", "Bold", 75, 14);
        let k3 = RawFontCacheKey::new("Arial", "Regular", 50, 14);
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    fn make_test_run(glyph_index: u32, string_index: usize, string_end: usize) -> ShapedVisualRun {
        ShapedVisualRun {
            glyphs: vec![make_glyph(glyph_index, string_index, 0.0, 10.0)],
            clusters: vec![ShapedCluster { string_start: string_index, string_end, glyph_start: 0, glyph_end: 1 }],
            raw_font_key: RawFontCacheKey::new("Test", "", 50, 16),
            flags: RunFlags::empty(),
            source_string_start: string_index,
            source_string_end: string_end,
            baseline_y: 12.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_w: 10.0,
            visual_h: 16.0,
            texture_atlas_x: 0.0,
            texture_atlas_y: 0.0,
            texture_atlas_w: 10.0,
            texture_atlas_h: 16.0,
            texture_translate_x: 1.0,
            texture_translate_y: 1.0,
            qglyphrun_index: 0,
            para_text: None,
            qtextline_idx: None,
            paragraph_wrap_w: None,
            para_indent: None,
        }
    }

    #[test]
    fn test_reflow_visual_snapshot_unchanged() {
        let old_run = make_test_run(1, 0, 1);
        let mut new_run = old_run.clone();
        new_run.visual_x = 10.0;

        let snapshot = ReflowVisualSnapshot::new(vec![old_run], vec![new_run]);
        assert!(snapshot.can_reuse_texture_for_run(0));
        assert!(!snapshot.run_needs_crossfade(0));
    }

    #[test]
    fn test_reflow_visual_snapshot_changed_shaping() {
        let old_run = make_test_run(1, 0, 1);
        let mut new_run = old_run.clone();
        new_run.glyphs[0].glyph_index = 99;

        let snapshot = ReflowVisualSnapshot::new(vec![old_run], vec![new_run]);
        assert!(!snapshot.can_reuse_texture_for_run(0));
        assert!(snapshot.run_needs_crossfade(0));
    }

    #[test]
    fn test_reflow_visual_snapshot_string_range_mapping() {
        let old_run1 = make_test_run(1, 0, 3);
        let old_run2 = make_test_run(2, 3, 6);
        let new_run1 = make_test_run(1, 0, 3);
        let new_run2 = make_test_run(2, 3, 6);
        let snapshot = ReflowVisualSnapshot::new(
            vec![old_run1, old_run2],
            vec![new_run1, new_run2],
        );
        assert_eq!(snapshot.run_mapping.len(), 2);
        assert_eq!(snapshot.run_mapping[0].old_run_index, 0);
        assert_eq!(snapshot.run_mapping[0].new_run_index, 0);
        assert!(snapshot.run_mapping[0].same_shaping);
        assert_eq!(snapshot.run_mapping[1].old_run_index, 1);
        assert_eq!(snapshot.run_mapping[1].new_run_index, 1);
    }

    #[test]
    fn test_reflow_old_position_for_new() {
        let mut old_run = make_test_run(1, 0, 1);
        old_run.visual_x = 5.0;
        let mut new_run = old_run.clone();
        new_run.visual_x = 15.0;
        let snapshot = ReflowVisualSnapshot::new(vec![old_run], vec![new_run]);
        let old_pos = snapshot.old_position_for_new(0);
        assert!(old_pos.is_some());
        assert_eq!(old_pos.unwrap().0, 5.0);
    }

    impl ShapedCluster {
        fn glyph_count(&self) -> usize {
            self.glyph_end - self.glyph_start
        }
    }
}
