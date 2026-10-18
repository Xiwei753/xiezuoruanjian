// SPDX-License-Identifier: LGPL-2.1-or-later
// Format layout is based on KenLM `lm/binary_format.hh`,
// `lm/binary_format.cc`, and `lm/model_type.hh` (LGPL-2.1-or-later).
//! Checked decoder for the KenLM binary header and PROBING storage extents.

use std::ops::Range;

use super::{probing::TableLayout, KenlmError};

const MAGIC: &[u8] = b"mmap lm http://kheafield.com/code format version 5\n\0";
const SANITY_SIZE: usize = 88;
const FIXED_PARAMETERS_SIZE: usize = 20;
const COUNTS_OFFSET: usize = SANITY_SIZE + FIXED_PARAMETERS_SIZE;
const MAX_SUPPORTED_ORDER: usize = 10;
const PROBING_MODEL_TYPE: u32 = 0;
const SEARCH_VERSION: u32 = 0;
const VOCABULARY_VERSION: u32 = 0;

#[derive(Debug)]
pub(super) struct ModelLayout {
    pub(super) order: usize,
    pub(super) counts: Vec<u64>,
    pub(super) vocabulary_strings: Range<usize>,
    pub(super) unigrams: Range<usize>,
    pub(super) middle_tables: Vec<TableLayout>,
    pub(super) longest_table: TableLayout,
}

pub(super) fn parse(bytes: &[u8]) -> Result<ModelLayout, KenlmError> {
    if bytes.len() < SANITY_SIZE + FIXED_PARAMETERS_SIZE {
        return invalid("file is shorter than a KenLM binary header");
    }
    if bytes.get(..MAGIC.len()) != Some(MAGIC) {
        return invalid("missing KenLM mmap binary format version 5 magic");
    }
    check_zero_bytes(bytes, 52..56, "magic padding")?;

    // The sanity fields are the explicit endian/alignment check written by
    // KenLM. They reject incompatible host layouts before any offsets are used.
    check_u32(bytes, 56, 0, "header sanity zero")?;
    check_u32(bytes, 60, 1.0_f32.to_bits(), "header sanity one")?;
    check_u32(
        bytes,
        64,
        (-0.5_f32).to_bits(),
        "header sanity minus one half",
    )?;
    check_u32(bytes, 68, 1, "header sanity word index")?;
    check_u32(bytes, 72, u32::MAX, "header sanity max word index")?;
    check_u32(bytes, 76, 0, "header sanity padding")?;
    check_u64(bytes, 80, 1, "header sanity uint64")?;

    let order = usize::from(read_u8(bytes, SANITY_SIZE, "model order")?);
    if !(2..=MAX_SUPPORTED_ORDER).contains(&order) {
        return Err(KenlmError::UnsupportedOrder(order));
    }

    let probing_multiplier = read_f32(bytes, SANITY_SIZE + 4, "probing multiplier")?;
    if !probing_multiplier.is_finite() || probing_multiplier < 1.0 {
        return invalid("probing multiplier is not finite or is less than 1.0");
    }

    let model_type = read_u32(bytes, SANITY_SIZE + 8, "model type")?;
    if model_type != PROBING_MODEL_TYPE {
        return Err(KenlmError::UnsupportedModelType(model_type));
    }

    let has_vocabulary = read_u8(bytes, SANITY_SIZE + 12, "vocabulary flag")?;
    if has_vocabulary != 1 {
        return invalid("this reader requires the model's embedded vocabulary strings");
    }
    check_zero_bytes(
        bytes,
        SANITY_SIZE + 1..SANITY_SIZE + 4,
        "fixed header padding",
    )?;
    check_zero_bytes(
        bytes,
        SANITY_SIZE + 13..SANITY_SIZE + 16,
        "fixed header padding",
    )?;

    let search_version = read_u32(bytes, SANITY_SIZE + 16, "search version")?;
    if search_version != SEARCH_VERSION {
        return Err(KenlmError::UnsupportedSearchVersion(search_version));
    }

    let counts_end = COUNTS_OFFSET
        .checked_add(
            order
                .checked_mul(8)
                .ok_or_else(|| format_error("n-gram count size overflow"))?,
        )
        .ok_or_else(|| format_error("n-gram count offset overflow"))?;
    ensure_range(bytes, COUNTS_OFFSET..counts_end, "n-gram counts")?;
    let counts = (0..order)
        .map(|index| read_u64(bytes, COUNTS_OFFSET + index * 8, "n-gram count"))
        .collect::<Result<Vec<_>, _>>()?;
    let vocab_count = counts[0];
    if vocab_count == 0 || vocab_count > u64::from(u32::MAX) {
        return invalid("unigram count cannot be represented by KenLM's 32-bit word index");
    }

    let header_size = align8(counts_end)?;
    ensure_range(bytes, 0..header_size, "aligned header")?;

    // PROBING's vocabulary begins with {version: u32, bound: WordIndex}, then
    // a linear-probing table of 12-byte {hash: u64, word_id: u32} entries.
    let vocab_buckets = bucket_count(vocab_count, probing_multiplier)?;
    let vocab_table_bytes = checked_table_bytes(vocab_buckets, 12, "vocabulary table")?;
    let vocab_size = 8_usize
        .checked_add(vocab_table_bytes)
        .ok_or_else(|| format_error("vocabulary area size overflow"))?;
    let vocab_header_end = header_size
        .checked_add(8)
        .ok_or_else(|| format_error("vocabulary header offset overflow"))?;
    ensure_range(bytes, header_size..vocab_header_end, "vocabulary header")?;

    let vocabulary_version = read_u32(bytes, header_size, "vocabulary version")?;
    if vocabulary_version != VOCABULARY_VERSION {
        return Err(KenlmError::UnsupportedVocabularyVersion(vocabulary_version));
    }
    let vocabulary_bound = read_u32(bytes, header_size + 4, "vocabulary bound")?;
    if u64::from(vocabulary_bound) != vocab_count {
        return invalid("vocabulary bound does not match the unigram count");
    }

    let search_start = header_size
        .checked_add(vocab_size)
        .ok_or_else(|| format_error("search area offset overflow"))?;
    let unigram_bytes = checked_table_bytes(vocab_count + 1, 8, "unigram vector")?;
    let unigram_end = search_start
        .checked_add(unigram_bytes)
        .ok_or_else(|| format_error("unigram vector end overflow"))?;
    ensure_range(bytes, search_start..unigram_end, "unigram vector")?;
    let unigrams = search_start..unigram_end;

    let mut cursor = unigram_end;
    let mut middle_tables = Vec::with_capacity(order.saturating_sub(2));
    for ngram_order in 2..order {
        let count = counts[ngram_order - 1];
        let buckets = bucket_count(count, probing_multiplier)?;
        let end = append_table(cursor, bytes.len(), buckets, 16, "middle n-gram table")?;
        middle_tables.push(TableLayout {
            range: cursor..end,
            buckets: usize_from_u64(buckets, "middle table bucket count")?,
            entry_size: 16,
        });
        cursor = end;
    }

    let longest_buckets = bucket_count(counts[order - 1], probing_multiplier)?;
    let longest_end = append_table(
        cursor,
        bytes.len(),
        longest_buckets,
        12,
        "longest n-gram table",
    )?;
    let longest_table = TableLayout {
        range: cursor..longest_end,
        buckets: usize_from_u64(longest_buckets, "longest table bucket count")?,
        entry_size: 12,
    };

    // KenLM stores optional word strings directly after the complete search
    // area. For this target model the remaining 27,458 bytes contain 6,914
    // NUL-delimited strings in vocabulary-id order.
    ensure_range(
        bytes,
        longest_end..bytes.len(),
        "embedded vocabulary strings",
    )?;
    Ok(ModelLayout {
        order,
        counts,
        vocabulary_strings: longest_end..bytes.len(),
        unigrams,
        middle_tables,
        longest_table,
    })
}

fn bucket_count(entries: u64, multiplier: f32) -> Result<u64, KenlmError> {
    let minimum = entries
        .checked_add(1)
        .ok_or_else(|| format_error("hash table bucket count overflow"))?;
    let scaled = ((entries as f32) * multiplier) as u64;
    Ok(minimum.max(scaled))
}

fn checked_table_bytes(buckets: u64, entry_size: usize, what: &str) -> Result<usize, KenlmError> {
    let buckets = usize_from_u64(buckets, what)?;
    buckets
        .checked_mul(entry_size)
        .ok_or_else(|| format_error(format!("{what} byte size overflow")))
}

fn append_table(
    start: usize,
    file_len: usize,
    buckets: u64,
    entry_size: usize,
    what: &str,
) -> Result<usize, KenlmError> {
    let byte_len = checked_table_bytes(buckets, entry_size, what)?;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| format_error(format!("{what} end offset overflow")))?;
    if end > file_len {
        return invalid(format!("{what} extends beyond the model file"));
    }
    Ok(end)
}

fn align8(value: usize) -> Result<usize, KenlmError> {
    value
        .checked_add(7)
        .map(|aligned| aligned & !7)
        .ok_or_else(|| format_error("header alignment overflow"))
}

fn usize_from_u64(value: u64, what: &str) -> Result<usize, KenlmError> {
    usize::try_from(value).map_err(|_| format_error(format!("{what} does not fit this platform")))
}

fn ensure_range(bytes: &[u8], range: Range<usize>, what: &str) -> Result<(), KenlmError> {
    if range.start > range.end || range.end > bytes.len() {
        return invalid(format!("{what} lies outside the model file"));
    }
    Ok(())
}

fn check_u32(bytes: &[u8], offset: usize, expected: u32, what: &str) -> Result<(), KenlmError> {
    let got = read_u32(bytes, offset, what)?;
    if got != expected {
        return invalid(format!("{what} does not match the KenLM reference value"));
    }
    Ok(())
}

fn check_u64(bytes: &[u8], offset: usize, expected: u64, what: &str) -> Result<(), KenlmError> {
    let got = read_u64(bytes, offset, what)?;
    if got != expected {
        return invalid(format!("{what} does not match the KenLM reference value"));
    }
    Ok(())
}

fn check_zero_bytes(bytes: &[u8], range: Range<usize>, what: &str) -> Result<(), KenlmError> {
    ensure_range(bytes, range.clone(), what)?;
    if bytes[range].iter().any(|byte| *byte != 0) {
        return invalid(format!("{what} must contain only zero bytes"));
    }
    Ok(())
}

fn read_u8(bytes: &[u8], offset: usize, what: &str) -> Result<u8, KenlmError> {
    bytes
        .get(offset)
        .copied()
        .ok_or_else(|| format_error(format!("truncated {what}")))
}

fn read_u32(bytes: &[u8], offset: usize, what: &str) -> Result<u32, KenlmError> {
    let raw = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| format_error(format!("truncated {what}")))?;
    Ok(u32::from_le_bytes(
        raw.try_into().expect("checked 4-byte range"),
    ))
}

fn read_u64(bytes: &[u8], offset: usize, what: &str) -> Result<u64, KenlmError> {
    let raw = bytes
        .get(offset..offset.saturating_add(8))
        .ok_or_else(|| format_error(format!("truncated {what}")))?;
    Ok(u64::from_le_bytes(
        raw.try_into().expect("checked 8-byte range"),
    ))
}

fn read_f32(bytes: &[u8], offset: usize, what: &str) -> Result<f32, KenlmError> {
    Ok(f32::from_bits(read_u32(bytes, offset, what)?))
}

fn invalid<T>(message: impl Into<String>) -> Result<T, KenlmError> {
    Err(format_error(message))
}

fn format_error(message: impl Into<String>) -> KenlmError {
    KenlmError::InvalidFormat(message.into())
}
