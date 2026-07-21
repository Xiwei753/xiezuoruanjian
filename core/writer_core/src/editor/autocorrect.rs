//! # 自动纠错引擎模块
//!
//! 本模块实现了基于 Aho-Corasick 算法的文本自动纠错功能，用于检测和修正常见的
//! 拼写错误和打字错误。
//!
//! ## 主要功能
//!
//! - **模式匹配**: 使用 Aho-Corasick 多模式匹配算法高效扫描文本中的错误模式
//! - **错误纠正**: 根据预定义的字典自动提供纠正建议
//! - **位置追踪**: 记录每个错误在原文中的精确位置（起始和结束索引）
//!
//! ## 核心结构
//!
//! - `AutoCorrectEngine`: 自动纠错引擎，封装了 Aho-Corasick 匹配器和替换规则
//! - `TypoCorrection`: 表示单个拼写错误的纠正建议，包含位置信息和建议文本
//!
//! ## 依赖关系
//!
//! - `aho_corasick`: 高效的多模式字符串匹配库
//! - `serde`: 序列化支持，用于 JSON 交互
//!
//! ## 使用场景
//!
//! - 编辑器中的实时拼写检查
//! - 批量文本纠错处理
//! - 输入法候选词纠正

use aho_corasick::AhoCorasick;
use serde::Serialize;

/// 单个拼写错误的纠正建议。
///
/// `start_index`/`end_index` 为 UTF-8 byte offset（半开区间 [start, end)），
/// 与 Core 其余范围语义一致。
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TypoCorrection {
    /// 错误文本的起始位置（UTF-8 byte offset，含）
    pub start_index: usize,
    /// 错误文本的结束位置（UTF-8 byte offset，不含，半开区间）
    pub end_index: usize,
    /// 原始错误文本
    pub original_text: String,
    /// 纠正建议文本
    pub suggestion: String,
}

/// 自动纠错引擎——基于 Aho-Corasick 多模式匹配。
///
/// 初始化时构建匹配器和替换规则，之后可反复调用 `scan_text` 扫描文本。
/// 不持有可变状态，线程安全（只要求 &self）。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_text_basic() {
        let dict = vec![
            ("teh".to_string(), "the".to_string()),
            ("donot".to_string(), "do not".to_string()),
        ];
        let engine = AutoCorrectEngine::new(&dict).unwrap();
        let text = "teh quick brown fox donot jump.";
        let corrections = engine.scan_text(text);

        assert_eq!(corrections.len(), 2);

        assert_eq!(corrections[0].original_text, "teh");
        assert_eq!(corrections[0].suggestion, "the");
        assert_eq!(corrections[0].start_index, 0);
        assert_eq!(corrections[0].end_index, 3);

        assert_eq!(corrections[1].original_text, "donot");
        assert_eq!(corrections[1].suggestion, "do not");
        assert_eq!(corrections[1].start_index, 20);
        assert_eq!(corrections[1].end_index, 25);
    }

    #[test]
    fn test_scan_text_no_matches() {
        let dict = vec![
            ("teh".to_string(), "the".to_string()),
        ];
        let engine = AutoCorrectEngine::new(&dict).unwrap();
        let text = "the quick brown fox";
        let corrections = engine.scan_text(text);
        assert_eq!(corrections.len(), 0);
    }

    #[test]
    fn test_scan_text_empty_input() {
        let dict = vec![
            ("teh".to_string(), "the".to_string()),
        ];
        let engine = AutoCorrectEngine::new(&dict).unwrap();
        let text = "";
        let corrections = engine.scan_text(text);
        assert_eq!(corrections.len(), 0);
    }

    #[test]
    fn test_scan_text_multiple_occurrences() {
        let dict = vec![
            ("teh".to_string(), "the".to_string()),
        ];
        let engine = AutoCorrectEngine::new(&dict).unwrap();
        let text = "teh teh teh";
        let corrections = engine.scan_text(text);
        assert_eq!(corrections.len(), 3);
        assert_eq!(corrections[0].start_index, 0);
        assert_eq!(corrections[1].start_index, 4);
        assert_eq!(corrections[2].start_index, 8);
    }

    #[test]
    fn test_scan_text_unicode() {
        let dict = vec![
            ("錯字".to_string(), "错字".to_string()),
        ];
        let engine = AutoCorrectEngine::new(&dict).unwrap();
        let text = "这是一个錯字";
        let corrections = engine.scan_text(text);

        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].original_text, "錯字");
        assert_eq!(corrections[0].suggestion, "错字");
        // start_index and end_index are in bytes
        assert_eq!(&text[corrections[0].start_index..corrections[0].end_index], "錯字");
    }

    #[test]
    fn test_scan_text_case_sensitivity() {
        let dict = vec![
            ("Teh".to_string(), "The".to_string()),
        ];
        let engine = AutoCorrectEngine::new(&dict).unwrap();

        let text1 = "Teh";
        let corrections1 = engine.scan_text(text1);
        assert_eq!(corrections1.len(), 1);

        let text2 = "teh";
        let corrections2 = engine.scan_text(text2);
        assert_eq!(corrections2.len(), 0);
    }
}
