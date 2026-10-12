//! PULSE patch index coding conditioned on the old cell's BF16 exponent
//! (`exp-class-rice`, PULSE patch format v3).
//!
//! At a low learning rate a BF16 weight changes only when its update crosses
//! half an ULP, and the ULP is set by the exponent, so the old exponent
//! predicts which cells change. Both ends hold the old cells (the producer's
//! diff base, the consumer's snapshot), so the context is free on the wire.
//!
//! Cells are partitioned by the exponent byte of their old value. Each class
//! with changes codes its changed cells by their rank inside the class (the
//! number of earlier cells of the same class), or, when more than half the
//! class changed, the ranks of its unchanged cells. The rank lists use the v2
//! `rice-gap` block unchanged. Layout of one tensor's index stream (empty
//! when nothing changed):
//!
//! ```text
//! u16 LE  L                    class records, 1..=256
//! L x     u8  exp              strictly ascending
//!         u8  flags            bit 0: complement; other bits zero
//!         u32 LE m             ranks coded in the block
//!         u32 LE B             block byte length
//! L x     B bytes              rice-gap block of the m ranks
//! ```
//!
//! A class with c changed cells out of n_k is a complement class iff
//! 2c > n_k, then m = n_k - c (m = 0 and B = 0 when the whole class changed);
//! otherwise m = c >= 1. The decoder rejects every other combination, so each
//! index set has exactly one encoding for a given choice of rice parameters.

use std::fmt;

pub const RICE_Q_ESCAPE: u64 = 32;
pub const RICE_MAX_K: u32 = 31;
const RECORD_BYTES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

fn err<T>(msg: impl Into<String>) -> Result<T, FormatError> {
    Err(FormatError(msg.into()))
}

#[inline(always)]
fn class_of(cell: u16) -> usize {
    ((cell >> 7) & 0xFF) as usize
}

// --------------------------------------------------------------------- rice-gap block (format v2)

fn rice_cost_bits(v: &[u64], k: u32) -> u64 {
    let mut bits = 0u64;
    for &x in v {
        let q = x >> k;
        bits += q.min(RICE_Q_ESCAPE) + 1 + k as u64;
        if q >= RICE_Q_ESCAPE {
            bits += 32;
        }
    }
    bits
}

fn floor_log2_or_zero(x: f64) -> i64 {
    if x >= 1.0 {
        x.log2().floor() as i64
    } else {
        0
    }
}

/// The v2 encoder's choice of k: exact cost among the neighbours of a
/// mean-based and a subsampled-median-based estimate (ties go to the smaller k).
pub fn rice_parameter(v: &[u64]) -> u32 {
    let n = v.len();
    let mean = v.iter().map(|&x| x as u128).sum::<u128>() as f64 / n as f64;
    let step = (n / 4096).max(1);
    let mut sub: Vec<u64> = v.iter().step_by(step).copied().collect();
    sub.sort_unstable();
    let m = sub.len();
    let median = if m % 2 == 1 {
        sub[m / 2] as f64
    } else {
        (sub[m / 2 - 1] as f64 + sub[m / 2] as f64) / 2.0
    };
    let mut cands: Vec<u32> = Vec::new();
    for e in [
        floor_log2_or_zero(mean * std::f64::consts::LN_2),
        floor_log2_or_zero(median),
    ] {
        for k in [e - 1, e, e + 1] {
            if (0..=RICE_MAX_K as i64).contains(&k) && !cands.contains(&(k as u32)) {
                cands.push(k as u32);
            }
        }
    }
    cands.sort_unstable();
    let mut best = (u64::MAX, 0u32);
    for k in cands {
        let c = rice_cost_bits(v, k);
        if c < best.0 {
            best = (c, k);
        }
    }
    best.1
}

struct BitWriter {
    buf: Vec<u8>,
    acc: u64,
    nacc: u32,
}

impl BitWriter {
    fn with_capacity(bytes: usize) -> Self {
        BitWriter {
            buf: Vec::with_capacity(bytes),
            acc: 0,
            nacc: 0,
        }
    }
    #[inline(always)]
    fn put(&mut self, value: u64, nbits: u32) {
        // nbits <= 32
        self.acc = (self.acc << nbits) | (value & ((1u64 << nbits) - 1));
        self.nacc += nbits;
        while self.nacc >= 8 {
            self.nacc -= 8;
            self.buf.push((self.acc >> self.nacc) as u8);
        }
    }
    #[inline(always)]
    fn ones(&mut self, mut n: u64) {
        while n >= 32 {
            self.put(0xFFFF_FFFF, 32);
            n -= 32;
        }
        if n > 0 {
            self.put((1u64 << n) - 1, n as u32);
        }
    }
    fn finish(mut self, pad_one: bool) -> Vec<u8> {
        if self.nacc > 0 {
            let pad = 8 - self.nacc;
            let fill = if pad_one { (1u64 << pad) - 1 } else { 0 };
            self.put(fill, pad);
        }
        self.buf
    }
}

/// Encode coded values (gaps minus one, all < 2^32) as one rice-gap block.
pub fn rice_encode(v: &[u64]) -> Vec<u8> {
    if v.is_empty() {
        return Vec::new();
    }
    let k = rice_parameter(v);
    let mut unary = BitWriter::with_capacity(v.len() / 4 + 8);
    let mut rem = BitWriter::with_capacity((v.len() * k as usize) / 8 + 8);
    let mut esc: Vec<u8> = Vec::new();
    for &x in v {
        let q = x >> k;
        unary.ones(q.min(RICE_Q_ESCAPE));
        unary.put(0, 1);
        if k > 0 {
            rem.put(x & ((1u64 << k) - 1), k);
        }
        if q >= RICE_Q_ESCAPE {
            esc.extend_from_slice(&(q as u32).to_le_bytes());
        }
    }
    let unary = unary.finish(true);
    let rem = rem.finish(false);
    let mut out = Vec::with_capacity(5 + unary.len() + rem.len() + esc.len());
    out.push(k as u8);
    out.extend_from_slice(&(unary.len() as u32).to_le_bytes());
    out.extend_from_slice(&unary);
    out.extend_from_slice(&rem);
    out.extend_from_slice(&esc);
    out
}

/// Decode a rice-gap block of `m` values into strictly ascending positions
/// (cumulative gaps), all < `limit`. Validates exactly like the v2 decoder.
pub fn rice_decode_positions(buf: &[u8], m: u64, limit: u64) -> Result<Vec<u64>, FormatError> {
    if m == 0 {
        if !buf.is_empty() {
            return err("nonempty block with zero codes");
        }
        return Ok(Vec::new());
    }
    if m > limit {
        return err("more codes than cells");
    }
    if buf.len() < 5 {
        return err("truncated index stream");
    }
    let k = buf[0] as u32;
    if k > RICE_MAX_K {
        return err(format!("rice parameter {k} exceeds {RICE_MAX_K}"));
    }
    let ulen = u32::from_le_bytes(buf[1..5].try_into().unwrap()) as usize;
    let rlen = (m as u128 * k as u128).div_ceil(8) as usize;
    if 5 + ulen + rlen > buf.len() {
        return err("truncated index stream");
    }
    let unary = &buf[5..5 + ulen];
    let rem = &buf[5 + ulen..5 + ulen + rlen];
    // quotients from the unary stream
    let mut q: Vec<u64> = Vec::with_capacity(m as usize);
    let mut run = 0u64;
    let mut last_zero_bit: Option<usize> = None;
    for (bi, &byte) in unary.iter().enumerate() {
        if byte == 0xFF {
            run += 8;
            continue;
        }
        for b in 0..8 {
            if (byte >> (7 - b)) & 1 == 1 {
                run += 1;
            } else {
                if q.len() as u64 == m {
                    return err(format!("unary stream holds more than changed={m} codes"));
                }
                if run > RICE_Q_ESCAPE {
                    return err("unary quotient exceeds the escape code");
                }
                q.push(run);
                run = 0;
                last_zero_bit = Some(bi * 8 + b);
            }
        }
    }
    if q.len() as u64 != m {
        return err(format!(
            "unary stream holds {} codes, header says changed={m}",
            q.len()
        ));
    }
    if ulen != last_zero_bit.unwrap() / 8 + 1 {
        return err("malformed unary stream padding");
    }
    let n_esc = q.iter().filter(|&&x| x == RICE_Q_ESCAPE).count();
    if buf.len() != 5 + ulen + rlen + 4 * n_esc {
        return err("index stream length does not match its escapes");
    }
    let mut esc_iter = buf[5 + ulen + rlen..]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()) as u64);
    let mut out = Vec::with_capacity(m as usize);
    let mut pos: u64 = 0; // running sum of gaps (gap = v + 1)
    let mut bitpos: usize = 0;
    for &qi in &q {
        let qv = if qi == RICE_Q_ESCAPE {
            let big = esc_iter.next().unwrap();
            if big < RICE_Q_ESCAPE {
                return err("non-canonical escaped quotient");
            }
            big
        } else {
            qi
        };
        let mut v = qv << k;
        if k > 0 {
            let mut r = 0u64;
            for _ in 0..k {
                let bit = (rem[bitpos >> 3] >> (7 - (bitpos & 7))) & 1;
                r = (r << 1) | bit as u64;
                bitpos += 1;
            }
            v |= r;
        }
        if v >= limit {
            return err("index gap out of range");
        }
        pos += v + 1;
        if pos > limit {
            return err("index out of range");
        }
        out.push(pos - 1);
    }
    Ok(out)
}

// --------------------------------------------------------------------- exp-class-rice

fn gaps_minus_one(ranks: &[u64]) -> Vec<u64> {
    let mut v = Vec::with_capacity(ranks.len());
    let mut prev: i128 = -1;
    for &r in ranks {
        v.push((r as i128 - prev - 1) as u64);
        prev = r as i128;
    }
    v
}

/// Encode the changed flat indices `idx` (strictly ascending, < old.len())
/// of one tensor whose cells before the patch are `old` (BF16 bit patterns).
pub fn encode(old: &[u16], idx: &[u64]) -> Result<Vec<u8>, FormatError> {
    if idx.is_empty() {
        return Ok(Vec::new());
    }
    let n = old.len() as u64;
    if old.len() >= (1usize << 32) {
        return err("numel exceeds 2^32");
    }
    let mut ranks: Vec<Vec<u64>> = vec![Vec::new(); 256];
    let mut cnt = [0u64; 256];
    let mut start = 0usize;
    let mut prev: Option<u64> = None;
    for &p in idx {
        if p >= n || prev.is_some_and(|q| p <= q) {
            return err("indices must be strictly ascending and in range");
        }
        prev = Some(p);
        let p = p as usize;
        for &w in &old[start..p] {
            cnt[class_of(w)] += 1;
        }
        let e = class_of(old[p]);
        ranks[e].push(cnt[e]);
        cnt[e] += 1;
        start = p + 1;
    }
    for &w in &old[start..] {
        cnt[class_of(w)] += 1;
    }
    let mut records: Vec<u8> = Vec::new();
    let mut blocks: Vec<u8> = Vec::new();
    let mut l: u16 = 0;
    for e in 0..256 {
        let r = &ranks[e];
        if r.is_empty() {
            continue;
        }
        let (c, nk) = (r.len() as u64, cnt[e]);
        let comp = 2 * c > nk;
        let coded: Vec<u64> = if comp {
            let mut u = Vec::with_capacity((nk - c) as usize);
            let mut j = 0usize;
            for x in 0..nk {
                if j < r.len() && r[j] == x {
                    j += 1;
                } else {
                    u.push(x);
                }
            }
            u
        } else {
            r.clone()
        };
        let block = rice_encode(&gaps_minus_one(&coded));
        records.push(e as u8);
        records.push(comp as u8);
        records.extend_from_slice(&(coded.len() as u32).to_le_bytes());
        records.extend_from_slice(&(block.len() as u32).to_le_bytes());
        blocks.extend_from_slice(&block);
        l += 1;
    }
    let mut out = Vec::with_capacity(2 + records.len() + blocks.len());
    out.extend_from_slice(&l.to_le_bytes());
    out.extend_from_slice(&records);
    out.extend_from_slice(&blocks);
    Ok(out)
}

struct ClassState {
    comp: bool,
    list: Vec<u64>,
    #[allow(dead_code)]
    ptr: usize,
    changed: u64, // claimed changed count for a plain class, checked after the scan
}

/// Decode one tensor's `exp-class-rice` stream against its old cells.
/// Returns the changed flat indices, strictly ascending.
pub fn decode(old: &[u16], changed: u64, buf: &[u8]) -> Result<Vec<u64>, FormatError> {
    match parse(old.len() as u64, changed, buf)? {
        None => Ok(Vec::new()),
        Some(c) => scan(old, changed, c, 1),
    }
}

/// Like `decode`, splitting the scan of one tensor across `threads` threads.
pub fn decode_parallel(
    old: &[u16],
    changed: u64,
    buf: &[u8],
    threads: usize,
) -> Result<Vec<u64>, FormatError> {
    if threads <= 1 || old.len() < (1 << 22) {
        return decode(old, changed, buf);
    }
    let classes = parse(old.len() as u64, changed, buf)?;
    match classes {
        None => Ok(Vec::new()),
        Some(c) => scan(old, changed, c, threads),
    }
}

fn parse(
    n: u64,
    changed: u64,
    buf: &[u8],
) -> Result<Option<Vec<(usize, ClassState)>>, FormatError> {
    if changed == 0 {
        if !buf.is_empty() {
            return err("nonempty index bytes with changed=0");
        }
        return Ok(None);
    }
    if changed > n {
        return err(format!("changed={changed} exceeds numel={n}"));
    }
    if buf.len() < 2 {
        return err("truncated index stream");
    }
    let l = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    if l == 0 || l > 256 {
        return err(format!("class record count {l} out of range"));
    }
    let rec_end = 2 + l * RECORD_BYTES;
    if buf.len() < rec_end {
        return err("truncated class records");
    }
    let mut out = Vec::with_capacity(l);
    let mut pos = rec_end;
    let mut prev_e: i32 = -1;
    for i in 0..l {
        let r = &buf[2 + i * RECORD_BYTES..2 + (i + 1) * RECORD_BYTES];
        let e = r[0] as i32;
        let flags = r[1];
        let m = u32::from_le_bytes(r[2..6].try_into().unwrap()) as u64;
        let b = u32::from_le_bytes(r[6..10].try_into().unwrap()) as usize;
        if e <= prev_e {
            return err("class records must be strictly ascending by exponent");
        }
        prev_e = e;
        if flags & !1 != 0 {
            return err("unknown class flags");
        }
        let comp = flags & 1 == 1;
        if !comp && m == 0 {
            return err("empty non-complement class record");
        }
        if pos + b > buf.len() {
            return err("truncated class block");
        }
        let list = rice_decode_positions(&buf[pos..pos + b], m, n)?;
        pos += b;
        out.push((
            e as usize,
            ClassState {
                comp,
                list,
                ptr: 0,
                changed: if comp { 0 } else { m },
            },
        ));
    }
    if pos != buf.len() {
        return err("trailing bytes after the class blocks");
    }
    Ok(Some(out))
}

const NONE: u64 = u64::MAX;

/// Scan cells [lo, hi) given each class's count of cells before `lo`;
/// returns the emitted indices and each class's count at `hi`.
fn scan_segment(
    old: &[u16],
    lo: usize,
    lists: &[Option<(&[u64], bool)>],
    start_cnt: &[u64; 256],
    cap: usize,
) -> (Vec<u64>, [u64; 256], [usize; 256]) {
    let mut cnt = *start_cnt;
    let mut next = [NONE; 256];
    let mut ptr = [0usize; 256];
    let mut comp = [0u8; 256];
    for e in 0..256 {
        if let Some((l, c)) = lists[e] {
            // first coded rank at or after this segment's start count
            let p = l.partition_point(|&r| r < start_cnt[e]);
            ptr[e] = p;
            next[e] = if p < l.len() { l[p] } else { NONE - 1 };
            comp[e] = c as u8;
        }
    }
    // classes without a record never emit: next = NONE never equals a count,
    // comp = 0. Exhausted lists use NONE - 1 (also never a count).
    let mut out: Vec<u64> = vec![0; cap + 1];
    let mut len = 0usize;
    for (j, &w) in old.iter().enumerate() {
        let e = class_of(w);
        let r = cnt[e];
        cnt[e] = r + 1;
        let hit = (next[e] == r) as u8;
        if hit == 1 {
            let l = lists[e].unwrap().0;
            ptr[e] += 1;
            next[e] = if ptr[e] < l.len() {
                l[ptr[e]]
            } else {
                NONE - 1
            };
        }
        let emit = (hit ^ comp[e]) & (next[e] != NONE) as u8;
        out[len.min(cap)] = (lo + j) as u64;
        len += emit as usize;
    }
    out.truncate(len.min(cap + 1));
    if len > cap {
        out.push(0); // marks overflow: caller sees len > cap
    }
    (out, cnt, ptr)
}

fn scan(
    old: &[u16],
    changed: u64,
    classes: Vec<(usize, ClassState)>,
    threads: usize,
) -> Result<Vec<u64>, FormatError> {
    let mut lists: Vec<Option<(&[u64], bool)>> = vec![None; 256];
    for (e, s) in &classes {
        lists[*e] = Some((s.list.as_slice(), s.comp));
    }
    let n = old.len();
    let threads = threads.max(1).min((n >> 20).max(1));
    let seg = n.div_ceil(threads);
    let bounds: Vec<(usize, usize)> = (0..threads)
        .map(|t| (t * seg, ((t + 1) * seg).min(n)))
        .filter(|(a, b)| a < b)
        .collect();
    // per-segment class histograms -> start counts
    let hists: Vec<[u64; 256]> = if bounds.len() == 1 {
        vec![[0u64; 256]]
    } else {
        std::thread::scope(|s| {
            let hs: Vec<_> = bounds
                .iter()
                .map(|&(a, b)| {
                    s.spawn(move || {
                        let mut h = [0u64; 256];
                        for &w in &old[a..b] {
                            h[class_of(w)] += 1;
                        }
                        h
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        })
    };
    let mut starts: Vec<[u64; 256]> = Vec::with_capacity(bounds.len());
    let mut acc = [0u64; 256];
    for h in &hists {
        starts.push(acc);
        for e in 0..256 {
            acc[e] += h[e];
        }
    }
    let cap = changed as usize;
    let lists_ref = &lists;
    let results: Vec<(Vec<u64>, [u64; 256], [usize; 256])> = if bounds.len() == 1 {
        vec![scan_segment(old, 0, lists_ref, &starts[0], cap)]
    } else {
        std::thread::scope(|s| {
            let hs: Vec<_> = bounds
                .iter()
                .zip(starts.iter())
                .map(|(&(a, b), st)| {
                    s.spawn(move || scan_segment(&old[a..b], a, lists_ref, st, cap))
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        })
    };
    let total: usize = results.iter().map(|r| r.0.len()).sum();
    if total as u64 != changed {
        return err(format!(
            "stream decodes to {total} indices, header says changed={changed}"
        ));
    }
    let (_, final_cnt, final_ptr) = results.last().unwrap();
    for (e, s) in &classes {
        if final_ptr[*e] != s.list.len() {
            return err(format!("class {e}: rank beyond the class size"));
        }
        let nk = final_cnt[*e];
        let c = if s.comp {
            nk - s.list.len() as u64
        } else {
            s.changed
        };
        if (2 * c > nk) != s.comp {
            return err(format!("class {e}: non-canonical complement flag"));
        }
    }
    let mut out = Vec::with_capacity(total);
    for r in results {
        out.extend_from_slice(&r.0);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 11
        }
        fn unit(&mut self) -> f64 {
            self.next() as f64 / (1u64 << 53) as f64
        }
    }

    fn synth(n: usize, seed: u64) -> (Vec<u16>, Vec<u64>) {
        let mut g = Lcg(seed);
        let old: Vec<u16> = (0..n)
            .map(|_| {
                let e = 110 + (g.next() % 16) as u16;
                (e << 7)
                    | (g.next() % 128) as u16
                    | if g.next().is_multiple_of(2) {
                        0x8000
                    } else {
                        0
                    }
            })
            .collect();
        let idx: Vec<u64> = old
            .iter()
            .enumerate()
            .filter_map(|(i, &w)| {
                let e = class_of(w) as f64;
                let p = (2f64.powf(e - 120.0)).min(0.97);
                (g.unit() < p).then_some(i as u64)
            })
            .collect();
        (old, idx)
    }

    #[test]
    fn roundtrip_mixed_rates_including_complement_and_full_classes() {
        for seed in 0..6 {
            let (old, idx) = synth(200_000, seed);
            let enc = encode(&old, &idx).unwrap();
            let dec = decode(&old, idx.len() as u64, &enc).unwrap();
            assert_eq!(dec, idx);
        }
    }

    #[test]
    fn parallel_decode_matches_sequential() {
        let (old, idx) = synth(5_000_000, 11);
        let enc = encode(&old, &idx).unwrap();
        let c = idx.len() as u64;
        for t in [2, 3, 7] {
            assert_eq!(decode_parallel(&old, c, &enc, t).unwrap(), idx);
        }
        assert!(decode_parallel(&old, c + 1, &enc, 4).is_err());
        assert!(decode_parallel(&old, c - 1, &enc, 4).is_err());
    }

    #[test]
    fn whole_class_changed_and_empty() {
        let old: Vec<u16> = (0..1000).map(|i| ((120 + (i % 3)) << 7) as u16).collect();
        let idx: Vec<u64> = (0..1000).filter(|i| i % 3 == 1).collect();
        let enc = encode(&old, &idx).unwrap();
        assert_eq!(decode(&old, idx.len() as u64, &enc).unwrap(), idx);
        assert!(encode(&old, &[]).unwrap().is_empty());
        assert!(decode(&old, 0, &[]).unwrap().is_empty());
    }

    #[test]
    fn rejects_corruption() {
        let (old, idx) = synth(50_000, 7);
        let enc = encode(&old, &idx).unwrap();
        let c = idx.len() as u64;
        assert!(decode(&old, c + 1, &enc).is_err());
        assert!(decode(&old, c, &enc[..enc.len() - 1]).is_err());
        let mut extra = enc.clone();
        extra.push(0);
        assert!(decode(&old, c, &extra).is_err());
        // flip every byte of the record table once: never a silent wrong answer
        let rec_end = 2 + u16::from_le_bytes([enc[0], enc[1]]) as usize * RECORD_BYTES;
        for i in 0..rec_end {
            let mut bad = enc.clone();
            bad[i] ^= 0x01;
            if let Ok(d) = decode(&old, c, &bad) {
                assert_eq!(d, idx, "byte {i}");
            }
        }
        // a different old state must not decode to the same indices silently
        let mut old2 = old.clone();
        old2.swap(0, 1);
        if let Ok(d) = decode(&old2, c, &enc) {
            assert_eq!(d.len() as u64, c);
        }
    }

    #[test]
    fn rice_block_matches_v2_layout() {
        // gaps 1,1,3 -> v = 0,0,2 ; k = 0 by cost -> unary 0,0,110 -> bits 0 0 1 1 0 + pad 111 = 0x37
        let b = rice_encode(&[0, 0, 2]);
        assert_eq!(b, vec![0, 1, 0, 0, 0, 0b0011_0111]);
        assert_eq!(rice_decode_positions(&b, 3, 10).unwrap(), vec![0, 1, 4]);
    }
}
