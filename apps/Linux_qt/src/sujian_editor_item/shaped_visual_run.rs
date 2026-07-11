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
pub(crate) struct ReflowVisualSnapshot {
    pub old_shaped_runs: Vec<ShapedVisualRun>,
    pub new_shaped_runs: Vec<ShapedVisualRun>,
    pub old_positions: Vec<(f64, f64, f64)>,
    pub new_positions: Vec<(f64, f64, f64)>,
    pub old_baselines: Vec<f64>,
    pub new_baselines: Vec<f64>,
    pub old_bounds: Vec<(f64, f64, f64, f64)>,
    pub new_bounds: Vec<(f64, f64, f64, f64)>,
    pub unchanged_run_identity: Vec<bool>,
    pub changed_shaping: Vec<bool>,
}

impl ReflowVisualSnapshot {
    pub fn new(
        old_shaped_runs: Vec<ShapedVisualRun>,
        new_shaped_runs: Vec<ShapedVisualRun>,
    ) -> Self {
        let old_count = old_shaped_runs.len();
        let new_count = new_shaped_runs.len();
        let max_count = old_count.max(new_count);

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

        let mut unchanged_run_identity = vec![false; max_count];
        let mut changed_shaping = vec![true; max_count];

        for i in 0..max_count {
            if i < old_count && i < new_count {
                let old_run = &old_shaped_runs[i];
                let new_run = &new_shaped_runs[i];

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

                unchanged_run_identity[i] = same_font && same_glyphs && same_string_range;
                changed_shaping[i] = !unchanged_run_identity[i];
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

    #[test]
    fn test_reflow_visual_snapshot_unchanged() {
        let font_key = RawFontCacheKey::new("Test", "", 50, 16);
        let run = ShapedVisualRun {
            glyphs: vec![make_glyph(1, 0, 0.0, 10.0)],
            clusters: vec![ShapedCluster { string_start: 0, string_end: 1, glyph_start: 0, glyph_end: 1 }],
            raw_font_key: font_key.clone(),
            flags: RunFlags::empty(),
            source_string_start: 0,
            source_string_end: 1,
            baseline_y: 12.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_w: 10.0,
            visual_h: 16.0,
            texture_atlas_x: 0.0,
            texture_atlas_y: 0.0,
            texture_atlas_w: 10.0,
            texture_atlas_h: 16.0,
            qglyphrun_index: 0,
            para_text: None,
            qtextline_idx: None,
            paragraph_wrap_w: None,
            para_indent: None,
        };
        let old_run = run.clone();
        let mut new_run = run.clone();
        new_run.visual_x = 10.0;

        let snapshot = ReflowVisualSnapshot::new(vec![old_run], vec![new_run]);
        assert!(snapshot.can_reuse_texture_for_run(0));
        assert!(!snapshot.run_needs_crossfade(0));
    }

    #[test]
    fn test_reflow_visual_snapshot_changed_shaping() {
        let font_key = RawFontCacheKey::new("Test", "", 50, 16);
        let old_run = ShapedVisualRun {
            glyphs: vec![make_glyph(1, 0, 0.0, 10.0)],
            clusters: vec![ShapedCluster { string_start: 0, string_end: 1, glyph_start: 0, glyph_end: 1 }],
            raw_font_key: font_key.clone(),
            flags: RunFlags::empty(),
            source_string_start: 0,
            source_string_end: 1,
            baseline_y: 12.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_w: 10.0,
            visual_h: 16.0,
            texture_atlas_x: 0.0,
            texture_atlas_y: 0.0,
            texture_atlas_w: 10.0,
            texture_atlas_h: 16.0,
            qglyphrun_index: 0,
            para_text: None,
            qtextline_idx: None,
            paragraph_wrap_w: None,
            para_indent: None,
        };
        let mut new_run = old_run.clone();
        new_run.glyphs[0].glyph_index = 99;

        let snapshot = ReflowVisualSnapshot::new(vec![old_run], vec![new_run]);
        assert!(!snapshot.can_reuse_texture_for_run(0));
        assert!(snapshot.run_needs_crossfade(0));
    }

    impl ShapedCluster {
        fn glyph_count(&self) -> usize {
            self.glyph_end - self.glyph_start
        }
    }
}
