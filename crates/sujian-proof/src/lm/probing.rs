// SPDX-License-Identifier: LGPL-2.1-or-later
// Probing layout and lookup semantics are based on KenLM
// `lm/search_hashed.hh`, `lm/value.hh`, and `util/probing_hash_table.hh`
// (LGPL-2.1-or-later).
//! Checked accessors for KenLM's unquantized probing hash tables.

use std::ops::Range;

use super::KenlmError;

#[derive(Debug, Clone)]
pub(super) struct TableLayout {
    pub(super) range: Range<usize>,
    pub(super) buckets: usize,
    pub(super) entry_size: usize,
}

pub(super) fn lookup_middle(
    bytes: &[u8],
    table: &TableLayout,
    key: u64,
) -> Result<Option<(f32, f32)>, KenlmError> {
    let Some(offset) = find_entry(bytes, table, key)? else {
        return Ok(None);
    };
    let stored_prob = read_u32(bytes, offset + 8, "middle n-gram probability")?;
    let backoff = read_u32(bytes, offset + 12, "middle n-gram backoff")?;
    // KenLM uses the stored probability's sign bit as its left-extension flag.
    // GenericProbingProxy::Prob forces that bit back on before returning the
    // original non-positive log10 probability.
    Ok(Some((
        f32::from_bits(stored_prob | 0x8000_0000),
        f32::from_bits(backoff),
    )))
}

pub(super) fn lookup_longest(
    bytes: &[u8],
    table: &TableLayout,
    key: u64,
) -> Result<Option<f32>, KenlmError> {
    let Some(offset) = find_entry(bytes, table, key)? else {
        return Ok(None);
    };
    Ok(Some(f32::from_bits(read_u32(
        bytes,
        offset + 8,
        "longest n-gram probability",
    )?)))
}

fn find_entry(bytes: &[u8], table: &TableLayout, key: u64) -> Result<Option<usize>, KenlmError> {
    if table.buckets == 0 || (table.entry_size != 12 && table.entry_size != 16) {
        return Err(invalid("invalid probing table dimensions"));
    }
    let expected_len = table
        .buckets
        .checked_mul(table.entry_size)
        .ok_or_else(|| invalid("probing table byte size overflow"))?;
    if table.range.end.checked_sub(table.range.start) != Some(expected_len)
        || table.range.end > bytes.len()
    {
        return Err(invalid("probing table lies outside the mapped model"));
    }

    let bucket_count = table.buckets as u64;
    let ideal = key % bucket_count;
    for probe in 0..table.buckets {
        let bucket = ((ideal + probe as u64) % bucket_count) as usize;
        let offset = table.range.start + bucket * table.entry_size;
        let stored_key = read_u64(bytes, offset, "probing table key")?;
        if stored_key == key {
            return Ok(Some(offset));
        }
        // KenLM uses zero as the empty-key sentinel and probes linearly.
        if stored_key == 0 {
            return Ok(None);
        }
    }
    Ok(None)
}

fn read_u32(bytes: &[u8], offset: usize, what: &str) -> Result<u32, KenlmError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| invalid(format!("{what} offset overflow")))?;
    let raw = bytes
        .get(offset..end)
        .ok_or_else(|| invalid(format!("truncated {what}")))?;
    Ok(u32::from_le_bytes(
        raw.try_into().expect("checked 4-byte range"),
    ))
}

fn read_u64(bytes: &[u8], offset: usize, what: &str) -> Result<u64, KenlmError> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| invalid(format!("{what} offset overflow")))?;
    let raw = bytes
        .get(offset..end)
        .ok_or_else(|| invalid(format!("truncated {what}")))?;
    Ok(u64::from_le_bytes(
        raw.try_into().expect("checked 8-byte range"),
    ))
}

fn invalid(message: impl Into<String>) -> KenlmError {
    KenlmError::InvalidFormat(message.into())
}
