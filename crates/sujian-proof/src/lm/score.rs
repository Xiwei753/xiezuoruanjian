// SPDX-License-Identifier: LGPL-2.1-or-later
// Query and backoff semantics are based on KenLM `lm/model.cc`,
// `lm/search_hashed.hh`, `lm/value.hh`, and `lm/blank.hh`
// (LGPL-2.1-or-later).
//! KenLM base-10 backoff scoring for token sequences.

use super::{
    binary::ModelLayout,
    probing::{lookup_longest, lookup_middle},
    KenlmError, Model,
};

pub(super) fn score_tokens(
    model: &Model,
    tokens: &[&str],
    bos: bool,
    eos: bool,
) -> Result<f64, KenlmError> {
    let bytes = model.mapping.as_bytes();
    let mut history = Vec::with_capacity(model.layout.order.saturating_sub(1));
    if bos {
        history.push(model.vocabulary.begin_sentence());
    }

    // KenLM returns f32 word scores and accumulates sentence scores in f32.
    // Convert to f64 only at the public API boundary.
    let mut total = 0.0_f32;
    for token in tokens {
        let word = model.vocabulary.id(token);
        total += score_word(bytes, &model.layout, &history, word)?;
        push_history(&mut history, word, model.layout.order);
    }
    if eos {
        total += score_word(
            bytes,
            &model.layout,
            &history,
            model.vocabulary.end_sentence(),
        )?;
    }

    Ok(f64::from(total))
}

fn push_history(history: &mut Vec<u32>, word: u32, order: usize) {
    let max_history = order.saturating_sub(1);
    if max_history == 0 {
        history.clear();
        return;
    }
    if history.len() == max_history {
        history.remove(0);
    }
    history.push(word);
}

fn score_word(
    bytes: &[u8],
    layout: &ModelLayout,
    history: &[u32],
    word: u32,
) -> Result<f32, KenlmError> {
    let (mut score, _) = unigram(bytes, layout, word)?;
    let context_limit = history.len().min(layout.order - 1);

    // Start with the unigram and extend one context word at a time. An exact
    // higher-order n-gram replaces the lower-order score; a miss adds that
    // context's KenLM backoff weight, preserving the ARPA backoff equation.
    for context_len in 1..=context_limit {
        let context = &history[history.len() - context_len..];
        let mut key = u64::from(context[0]);
        for context_word in &context[1..] {
            key = combine_word_hash(key, *context_word);
        }
        key = combine_word_hash(key, word);

        let ngram_order = context_len + 1;
        let found_probability = if ngram_order == layout.order {
            lookup_longest(bytes, &layout.longest_table, key)?
        } else {
            let table = layout
                .middle_tables
                .get(ngram_order - 2)
                .ok_or_else(|| invalid("model layout is missing a middle n-gram table"))?;
            lookup_middle(bytes, table, key)?.map(|(probability, _)| probability)
        };

        if let Some(probability) = found_probability {
            score = probability;
        } else {
            score += context_backoff(bytes, layout, history, context_len)?;
        }
    }

    Ok(score)
}

fn unigram(bytes: &[u8], layout: &ModelLayout, word: u32) -> Result<(f32, f32), KenlmError> {
    if u64::from(word) >= layout.counts[0] {
        return Err(invalid("vocabulary id is outside the unigram vector"));
    }
    let entry_offset = layout
        .unigrams
        .start
        .checked_add(
            usize::try_from(word)
                .map_err(|_| invalid("vocabulary id does not fit this platform"))?
                .checked_mul(8)
                .ok_or_else(|| invalid("unigram offset overflow"))?,
        )
        .ok_or_else(|| invalid("unigram offset overflow"))?;
    let prob_bits = read_u32(bytes, entry_offset, "unigram probability")?;
    let backoff_offset = entry_offset
        .checked_add(4)
        .ok_or_else(|| invalid("unigram backoff offset overflow"))?;
    let backoff_bits = read_u32(bytes, backoff_offset, "unigram backoff")?;
    Ok((
        f32::from_bits(prob_bits | 0x8000_0000),
        f32::from_bits(backoff_bits),
    ))
}

fn context_backoff(
    bytes: &[u8],
    layout: &ModelLayout,
    history: &[u32],
    context_len: usize,
) -> Result<f32, KenlmError> {
    let context = &history[history.len() - context_len..];
    if context_len == 1 {
        return Ok(unigram(bytes, layout, context[0])?.1);
    }

    let mut key = u64::from(context[0]);
    for context_word in &context[1..] {
        key = combine_word_hash(key, *context_word);
    }
    let Some(table) = layout.middle_tables.get(context_len - 2) else {
        return Err(invalid("model layout is missing a context backoff table"));
    };
    Ok(lookup_middle(bytes, table, key)?
        .map(|(_, backoff)| backoff)
        .unwrap_or(0.0))
}

fn combine_word_hash(current: u64, next: u32) -> u64 {
    current.wrapping_mul(8_978_948_897_894_561_157)
        ^ (u64::from(next) + 1).wrapping_mul(17_894_857_484_156_487_943)
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

fn invalid(message: impl Into<String>) -> KenlmError {
    KenlmError::InvalidFormat(message.into())
}
