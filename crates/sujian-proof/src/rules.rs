// SPDX-License-Identifier: GPL-3.0-only

use std::io::{self, BufRead};

use crate::segment::is_han;

// Rule entries are data, not a second language model. Literal particle rules
// are deliberately narrow; no general classifier guesses 的/地/得 from one
// character of context. Upstream pycorrector license applies only to behavior
// modeled in detector/corrector; the small default table below is curated data.

#[derive(Clone, Debug)]
pub struct RuleSet {
    literal_rules: Vec<LiteralRule>,
    enable_punctuation_runs: bool,
    enable_repeated_han: bool,
}

#[derive(Clone, Debug)]
struct LiteralRule {
    id: String,
    source: String,
    replacement: String,
    severity: String,
    exceptions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuleMatch {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) original: String,
    pub(crate) replacement: String,
    pub(crate) rule_id: String,
    pub(crate) severity: String,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self::with_common_rules()
    }
}

impl RuleSet {
    pub fn empty() -> Self {
        Self {
            literal_rules: Vec::new(),
            enable_punctuation_runs: false,
            enable_repeated_han: false,
        }
    }

    /// A compact, conservative baseline: a few high-signal fixed forms,
    /// repeated Chinese punctuation/characters as weak hints, and explicit
    /// particle examples. Applications can add licensed correction data with
    /// `load_tsv` instead of embedding upstream datasets in this crate.
    pub fn with_common_rules() -> Self {
        let mut rules = Self {
            literal_rules: Vec::new(),
            enable_punctuation_runs: true,
            enable_repeated_han: true,
        };
        rules.add_literal("fixed.should", "因该", "应该", "error", [] as [&str; 0]);
        rules.add_literal("fixed.already", "以经", "已经", "error", [] as [&str; 0]);
        rules.add_literal(
            "particle.dei.urgent",
            "着急的哭",
            "着急得哭",
            "warning",
            [] as [&str; 0],
        );
        rules.add_literal(
            "particle.dei.run",
            "跑的很快",
            "跑得很快",
            "warning",
            [] as [&str; 0],
        );
        rules.add_literal(
            "particle.dei.slow",
            "慢慢的走",
            "慢慢地走",
            "warning",
            [] as [&str; 0],
        );
        rules
    }

    pub fn add_literal<I, S>(
        &mut self,
        id: impl Into<String>,
        source: impl Into<String>,
        replacement: impl Into<String>,
        severity: impl Into<String>,
        exceptions: I,
    ) where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let source = source.into();
        let replacement = replacement.into();
        if source.is_empty() || source == replacement {
            return;
        }
        self.literal_rules.push(LiteralRule {
            id: id.into(),
            source,
            replacement,
            severity: normalize_severity(&severity.into()),
            exceptions: exceptions.into_iter().map(Into::into).collect(),
        });
        self.literal_rules.sort_by(|left, right| {
            right
                .source
                .chars()
                .count()
                .cmp(&left.source.chars().count())
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    /// Load TSV rows: `rule_id<TAB>source<TAB>replacement<TAB>severity`
    /// followed by zero or more context phrases that suppress the rule.
    /// Severity accepts `error`, `warning`, or `hint`.
    pub fn load_tsv<R: BufRead>(&mut self, reader: R) -> io::Result<()> {
        for (line_idx, line) in reader.lines().enumerate() {
            let line = line?;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() < 4 || fields[..4].iter().any(|field| field.is_empty()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "rule TSV line {} requires id, source, replacement, severity",
                        line_idx + 1
                    ),
                ));
            }
            let severity = fields[3].trim();
            if !matches!(severity, "error" | "warning" | "hint") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("rule TSV line {} has unknown severity", line_idx + 1),
                ));
            }
            self.add_literal(
                fields[0].trim(),
                fields[1],
                fields[2],
                severity,
                fields[4..]
                    .iter()
                    .map(|item| item.trim())
                    .filter(|item| !item.is_empty()),
            );
        }
        Ok(())
    }

    pub(crate) fn detect(&self, text: &str) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for rule in &self.literal_rules {
            for (start_byte, original) in text.match_indices(&rule.source) {
                if self.is_exempt(text, start_byte, original.len(), &rule.exceptions) {
                    continue;
                }
                matches.push(RuleMatch {
                    start_byte,
                    end_byte: start_byte + original.len(),
                    original: original.to_owned(),
                    replacement: rule.replacement.clone(),
                    rule_id: rule.id.clone(),
                    severity: rule.severity.clone(),
                });
            }
        }
        if self.enable_punctuation_runs {
            matches.extend(detect_repeated_punctuation(text));
        }
        if self.enable_repeated_han {
            matches.extend(detect_repeated_han(text));
        }
        matches.sort_by(|left, right| {
            severity_rank(&left.severity)
                .cmp(&severity_rank(&right.severity))
                .then_with(|| {
                    (right.end_byte - right.start_byte).cmp(&(left.end_byte - left.start_byte))
                })
                .then_with(|| left.start_byte.cmp(&right.start_byte))
                .then_with(|| left.rule_id.cmp(&right.rule_id))
        });
        matches
    }

    fn is_exempt(&self, text: &str, start: usize, len: usize, exceptions: &[String]) -> bool {
        if exceptions.is_empty() {
            return false;
        }
        let context_start = text[..start]
            .char_indices()
            .rev()
            .nth(8)
            .map(|(byte, _)| byte)
            .unwrap_or(0);
        let after_match = start + len;
        let context_end = text[after_match..]
            .char_indices()
            .nth(8)
            .map(|(byte, _)| after_match + byte)
            .unwrap_or(text.len());
        let context = &text[context_start..context_end];
        exceptions
            .iter()
            .any(|exception| context.contains(exception))
    }
}

fn detect_repeated_punctuation(text: &str) -> Vec<RuleMatch> {
    let mut result = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let (start, ch) = chars[index];
        if !matches!(
            ch,
            '，' | '。' | '！' | '？' | '；' | '：' | ',' | '!' | '?' | ';' | ':'
        ) {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < chars.len() && chars[end].1 == ch {
            end += 1;
        }
        if end - index > 1 {
            let end_byte = if end < chars.len() {
                chars[end].0
            } else {
                text.len()
            };
            let original = &text[start..end_byte];
            result.push(RuleMatch {
                start_byte: start,
                end_byte,
                original: original.to_owned(),
                replacement: ch.to_string(),
                rule_id: "punctuation.repeated".to_owned(),
                // Repeated punctuation can be expressive, so this stays a hint.
                severity: "hint".to_owned(),
            });
        }
        index = end;
    }
    result
}

fn detect_repeated_han(text: &str) -> Vec<RuleMatch> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let (start, ch) = chars[index];
        if !is_han(ch) {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < chars.len() && chars[end].1 == ch {
            end += 1;
        }
        if end - index > 1 {
            let end_byte = if end < chars.len() {
                chars[end].0
            } else {
                text.len()
            };
            let original = &text[start..end_byte];
            if !is_common_reduplication(original) {
                result.push(RuleMatch {
                    start_byte: start,
                    end_byte,
                    original: original.to_owned(),
                    replacement: ch.to_string(),
                    rule_id: "character.repeated".to_owned(),
                    severity: "warning".to_owned(),
                });
            }
        }
        index = end;
    }
    result
}

fn is_common_reduplication(word: &str) -> bool {
    const ANTI_RULES: &[&str] = &[
        "人人",
        "年年",
        "天天",
        "日日",
        "时时",
        "处处",
        "事事",
        "家家",
        "户户",
        "个个",
        "种种",
        "点点",
        "慢慢",
        "渐渐",
        "常常",
        "往往",
        "仅仅",
        "刚刚",
        "久久",
        "看看",
        "听听",
        "想想",
        "说说",
        "走走",
        "跳跳",
        "星星",
        "妹妹",
        "弟弟",
        "姐姐",
        "哥哥",
        "爸爸",
        "妈妈",
        "爷爷",
        "奶奶",
        "叔叔",
        "姑姑",
        "舅舅",
        "谢谢",
        "试试",
        "休息休息",
        "研究研究",
    ];
    ANTI_RULES.contains(&word)
}

fn normalize_severity(severity: &str) -> String {
    match severity {
        "error" | "warning" | "hint" => severity.to_owned(),
        _ => "warning".to_owned(),
    }
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    }
}
