use aho_corasick::AhoCorasick;
use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TypoCorrection {
    pub start_index: usize,
    pub end_index: usize,
    pub original_text: String,
    pub suggestion: String,
}

pub struct AutoCorrectEngine {
    matcher: AhoCorasick,
    replacements: Vec<String>,
}

impl AutoCorrectEngine {
    pub fn new(dictionary: &[(String, String)]) -> Result<Self, String> {
        let patterns: Vec<&str> = dictionary.iter().map(|(k, _)| k.as_str()).collect();
        let replacements: Vec<String> = dictionary.iter().map(|(_, v)| v.clone()).collect();

        let matcher = AhoCorasick::new(&patterns).map_err(|e| e.to_string())?;

        Ok(Self {
            matcher,
            replacements,
        })
    }

    pub fn scan_text(&self, text: &str) -> Vec<TypoCorrection> {
        self.matcher
            .find_iter(text)
            .map(|mat| {
                let start_index = mat.start();
                let end_index = mat.end();
                let pattern_index = mat.pattern().as_usize();

                let original_text = text[start_index..end_index].to_string();
                let suggestion = self.replacements[pattern_index].clone();

                TypoCorrection {
                    start_index,
                    end_index,
                    original_text,
                    suggestion,
                }
            })
            .collect()
    }
}
