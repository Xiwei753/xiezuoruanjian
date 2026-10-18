// SPDX-License-Identifier: LGPL-2.1-or-later
// Vocabulary serialization and lookup are based on KenLM `lm/vocab.hh` and
// `lm/vocab.cc` (LGPL-2.1-or-later). MurmurHash64A follows the implementation
// in `util/murmur_hash.cc`, whose header identifies it as public domain and
// MIT for business use.
//! KenLM's portable MurmurHash vocabulary lookup for embedded vocab strings.

use std::collections::HashMap;
use std::ops::Range;

use super::KenlmError;

pub(super) struct Vocabulary {
    ids_by_hash: HashMap<u64, u32>,
    begin_sentence: u32,
    end_sentence: u32,
}

impl Vocabulary {
    pub(super) fn id(&self, token: &str) -> u32 {
        self.ids_by_hash
            .get(&murmur_hash_64a(token.as_bytes()))
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn begin_sentence(&self) -> u32 {
        self.begin_sentence
    }

    pub(super) fn end_sentence(&self) -> u32 {
        self.end_sentence
    }
}

pub(super) fn parse(
    bytes: &[u8],
    range: Range<usize>,
    expected_count: u64,
) -> Result<Vocabulary, KenlmError> {
    let raw = bytes.get(range).ok_or_else(|| {
        KenlmError::InvalidFormat("embedded vocabulary range is truncated".to_owned())
    })?;
    if !raw.ends_with(&[0]) {
        return Err(KenlmError::InvalidFormat(
            "embedded vocabulary does not end with a NUL delimiter".to_owned(),
        ));
    }

    let expected_count = usize::try_from(expected_count).map_err(|_| {
        KenlmError::InvalidFormat("vocabulary count does not fit this platform".to_owned())
    })?;
    let mut words = raw.split(|byte| *byte == 0).collect::<Vec<_>>();
    if words.pop() != Some(&[][..]) || words.len() != expected_count {
        return Err(KenlmError::InvalidFormat(format!(
            "embedded vocabulary contains {} words; header declares {expected_count}",
            words.len()
        )));
    }
    if words.first().copied() != Some(&b"<unk>"[..]) {
        return Err(KenlmError::InvalidFormat(
            "embedded vocabulary must begin with KenLM's <unk> token".to_owned(),
        ));
    }

    let mut ids_by_hash = HashMap::with_capacity(expected_count.saturating_sub(1));
    let mut begin_sentence = None;
    let mut end_sentence = None;
    for (id, word) in words.iter().enumerate().skip(1) {
        if word.is_empty() {
            return Err(KenlmError::InvalidFormat(
                "embedded vocabulary contains an empty token".to_owned(),
            ));
        }
        let id = u32::try_from(id).map_err(|_| {
            KenlmError::InvalidFormat("vocabulary id exceeds KenLM's 32-bit range".to_owned())
        })?;
        if *word == &b"<s>"[..] {
            begin_sentence = Some(id);
        } else if *word == &b"</s>"[..] {
            end_sentence = Some(id);
        }

        // KenLM probes on this 64-bit hash rather than comparing strings. Keep
        // the first ID for collisions, matching linear probing's lookup order.
        ids_by_hash.entry(murmur_hash_64a(word)).or_insert(id);
    }

    let begin_sentence = begin_sentence.ok_or_else(|| {
        KenlmError::InvalidFormat("embedded vocabulary has no <s> marker".to_owned())
    })?;
    let end_sentence = end_sentence.ok_or_else(|| {
        KenlmError::InvalidFormat("embedded vocabulary has no </s> marker".to_owned())
    })?;

    // KenLM explicitly reserves these hashes as unknown, even if an unusual
    // vocabulary table happens to contain a matching spelling.
    ids_by_hash.remove(&murmur_hash_64a(b"<unk>"));
    ids_by_hash.remove(&murmur_hash_64a(b"<UNK>"));

    Ok(Vocabulary {
        ids_by_hash,
        begin_sentence,
        end_sentence,
    })
}

fn murmur_hash_64a(key: &[u8]) -> u64 {
    const MULTIPLIER: u64 = 0xc6a4_a793_5bd1_e995;
    const SHIFT: u32 = 47;

    let mut hash = (key.len() as u64).wrapping_mul(MULTIPLIER);
    let mut chunks = key.chunks_exact(8);
    for chunk in &mut chunks {
        let mut value = u64::from_le_bytes(chunk.try_into().expect("8-byte chunk"));
        value = value.wrapping_mul(MULTIPLIER);
        value ^= value >> SHIFT;
        value = value.wrapping_mul(MULTIPLIER);
        hash ^= value;
        hash = hash.wrapping_mul(MULTIPLIER);
    }

    let tail = chunks.remainder();
    for (index, byte) in tail.iter().enumerate() {
        hash ^= u64::from(*byte) << (index * 8);
    }
    if !tail.is_empty() {
        hash = hash.wrapping_mul(MULTIPLIER);
    }

    hash ^= hash >> SHIFT;
    hash = hash.wrapping_mul(MULTIPLIER);
    hash ^= hash >> SHIFT;
    hash
}
