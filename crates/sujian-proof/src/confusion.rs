// SPDX-License-Identifier: GPL-3.0-only
//! Aho-Corasick dictionary matching shared with the legacy Core adapter.

use aho_corasick::AhoCorasick;

/// One literal confusion-dictionary match in UTF-8 byte coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfusionMatch {
    pub start_byte: usize,
    pub end_byte: usize,
    pub original_text: String,
    pub suggestion: String,
}

/// Immutable multi-pattern matcher for literal confusion replacements.
pub struct ConfusionMatcher {
    matcher: AhoCorasick,
    replacements: Vec<String>,
}

impl ConfusionMatcher {
    pub fn new(dictionary: &[(String, String)]) -> Result<Self, String> {
        let patterns: Vec<&str> = dictionary.iter().map(|(key, _)| key.as_str()).collect();
        let replacements = dictionary
            .iter()
            .map(|(_, replacement)| replacement.clone())
            .collect();
        let matcher = AhoCorasick::new(&patterns).map_err(|error| error.to_string())?;
        Ok(Self {
            matcher,
            replacements,
        })
    }

    /// Find non-overlapping matches using Aho-Corasick's standard leftmost behavior.
    pub fn scan_text(&self, text: &str) -> Vec<ConfusionMatch> {
        self.matcher
            .find_iter(text)
            .map(|matched| {
                let start_byte = matched.start();
                let end_byte = matched.end();
                let pattern_index = matched.pattern().as_usize();
                ConfusionMatch {
                    start_byte,
                    end_byte,
                    original_text: text[start_byte..end_byte].to_string(),
                    suggestion: self.replacements[pattern_index].clone(),
                }
            })
            .collect()
    }
}
