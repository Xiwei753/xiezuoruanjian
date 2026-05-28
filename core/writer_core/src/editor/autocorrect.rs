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
