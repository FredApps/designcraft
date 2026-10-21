//! The record index of a PageMaker 4–7 stream (PC or Mac).
//!
//! The header's index descriptor (offset 44) points either at the index itself, a run of 16-byte
//! entries, or at a list of 10-byte block references, each naming a run of entries. An entry
//! names a record type, a count and the offset of that many fixed-size records. A "group" entry
//! (type 1, flag 3) instead points at 10-byte chunk references, each holding part of a long
//! list of records of its subtype: large stories, many pages, embedded images.
//!
//! See `FORMAT.md` for the layout as observed in sample files.

use crate::bytes::Bytes;

/// One list of records of one type, in file order: its chunks as (count, offset).
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub kind: u8,
    pub chunks: Vec<(usize, usize)>,
}

impl List {
    pub fn count(&self) -> usize {
        self.chunks.iter().map(|c| c.0).fold(0usize, usize::saturating_add)
    }

    /// The records, `size` bytes each; a record that would run past the end stops the list.
    pub fn records<'a>(&self, b: &Bytes<'a>, size: usize) -> Vec<&'a [u8]> {
        let mut out = Vec::new();
        for &(count, at) in &self.chunks {
            // A count can't promise more records than the file holds.
            let fits = b.len().saturating_sub(at) / size.max(1);
            for k in 0..count.min(fits) {
                match b.slice(at + k * size, size) {
                    Some(r) => out.push(r),
                    None => break,
                }
            }
        }
        out
    }

    /// The bytes of the records, `size` each, joined (text records are 1 byte each).
    pub fn bytes(&self, b: &Bytes<'_>, size: usize) -> Vec<u8> {
        self.records(b, size).concat()
    }
}

/// Index entry types, as observed.
pub mod kind {
    /// A group: its records are in chunks.
    pub const GROUP: u8 = 0x01;
    pub const PARA_RUNS: u8 = 0x0B;
    pub const TEXT: u8 = 0x0D;
    pub const FONTS: u8 = 0x13;
    pub const DOCUMENT: u8 = 0x18;
    pub const OBJECTS: u8 = 0x19;
    pub const STORY: u8 = 0x1B;
    pub const TRANSFORMS: u8 = 0x28;
    pub const CHAR_RUNS: u8 = 0x1C;
}

/// The most index entries read (real files have hundreds).
const MAX_ENTRIES: usize = 1 << 20;

/// The record lists of the index, in index order.
pub fn index(b: &Bytes<'_>) -> Vec<List> {
    let (Some(desc), Some(count), Some(at)) = (b.u8(45), b.u16(46), b.u32(48)) else { return vec![] };
    let count = usize::from(count);
    let at = at as usize;
    // Runs of 16-byte entries: the index itself, or the blocks a block list names.
    let runs: Vec<(usize, usize)> = match desc {
        0 => vec![(count, at)],
        _ => (0..count).filter_map(|k| Some((usize::from(b.u16(at + 10 * k + 2)?), b.u32(at + 10 * k + 4)? as usize))).collect(),
    };
    let mut out = Vec::new();
    for (n, start) in runs {
        for k in 0..n {
            if out.len() >= MAX_ENTRIES {
                return out;
            }
            let Some(e) = start.checked_add(k.saturating_mul(16)) else { break };
            let Some(entry) = b.slice(e, 16) else { break };
            // Every entry carries FF FF at 8; anything else is padding or damage.
            if entry.get(8..10) != Some(&[0xFF, 0xFF][..]) {
                continue;
            }
            let (Some(t), Some(c), Some(p), Some(flag), Some(sub)) = (b.u8(e + 1), b.u16(e + 2), b.u32(e + 4), b.u8(e + 10), b.u8(e + 11)) else {
                continue;
            };
            if t == kind::GROUP && flag == 3 {
                let chunks = (0..usize::from(c))
                    .filter_map(|j| {
                        let r = (p as usize).checked_add(10 * j)?;
                        (b.u8(r + 1)? == sub && b.slice(r + 8, 2)? == [0xFF, 0xFF]).then_some(())?;
                        Some((usize::from(b.u16(r + 2)?), b.u32(r + 4)? as usize))
                    })
                    .filter(|c| c.1 != 0)
                    .collect();
                out.push(List { kind: sub, chunks });
            } else {
                out.push(List { kind: t, chunks: if p == 0 { vec![] } else { vec![(usize::from(c), p as usize)] } });
            }
        }
    }
    out
}
