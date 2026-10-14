//! Line-level three-way merge (git xdl_merge style).
//!
//! Compute base→ours and base→theirs change chunks independently (LCS diff).
//! Non-overlapping chunks apply cleanly; overlapping chunks group into a
//! conflict region unless both sides made the identical change. Conflicts
//! render as git `merge.conflictStyle diff3` markers.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    /// Cleanly merged lines.
    Stable(Vec<String>),
    /// Both sides changed the region differently.
    Conflict {
        ours: Vec<String>,
        base: Vec<String>,
        theirs: Vec<String>,
    },
}

/// LCS alignment between two line sequences: pairs of (a_idx, b_idx), sorted.
fn lcs_pairs(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[at(i, j)] = if a[i] == b[j] {
                dp[at(i + 1, j + 1)] + 1
            } else {
                dp[at(i + 1, j)].max(dp[at(i, j + 1)])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if dp[at(i + 1, j)] >= dp[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// One change: base[b_lo..b_hi) was replaced by other[o_lo..o_hi).
#[derive(Clone, Copy)]
struct Chunk {
    b_lo: usize,
    b_hi: usize,
    o_lo: usize,
    o_hi: usize,
}

/// Change chunks of base→other, ordered by base range (disjoint).
fn diff_chunks(base: &[String], other: &[String], pairs: &[(usize, usize)]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let (mut bp, mut op) = (0usize, 0usize);
    for &(bi, oi) in pairs {
        if bi > bp || oi > op {
            chunks.push(Chunk { b_lo: bp, b_hi: bi, o_lo: op, o_hi: oi });
        }
        bp = bi + 1;
        op = oi + 1;
    }
    if bp < base.len() || op < other.len() {
        chunks.push(Chunk { b_lo: bp, b_hi: base.len(), o_lo: op, o_hi: other.len() });
    }
    chunks
}

/// Conflict-extent boundaries on the other side for a base range [g_lo, g_hi):
/// - from: position right after the last matched line with base < g_lo
/// - to:   position of the first matched line with base >= g_hi
/// (0 / other.len() when no such pair exists)
fn extent_from(pairs: &[(usize, usize)], g_lo: usize) -> usize {
    let mut from = 0usize;
    for &(b, x) in pairs {
        if b >= g_lo {
            break;
        }
        from = x + 1;
    }
    from
}

fn extent_to(pairs: &[(usize, usize)], g_hi: usize, other_len: usize) -> usize {
    for &(b, x) in pairs {
        if b >= g_hi {
            return x;
        }
    }
    other_len
}

/// Whether two base ranges conflict. Non-empty ranges must strictly overlap;
/// insertions at the same point conflict; an insertion strictly inside a
/// changed range conflicts; boundary-adjacent changes merge cleanly.
fn overlaps(a_lo: usize, a_hi: usize, b_lo: usize, b_hi: usize) -> bool {
    match (a_lo == a_hi, b_lo == b_hi) {
        (true, true) => a_lo == b_lo,
        (true, false) => a_lo > b_lo && a_lo < b_hi,
        (false, true) => b_lo > a_lo && b_lo < a_hi,
        (false, false) => a_lo < b_hi && b_lo < a_hi,
    }
}

fn split_lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    let mut lines: Vec<String> = s.split('\n').map(|l| l.to_string()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Chunk-based three-way merge over line sequences.
pub fn diff3(base: &[String], ours: &[String], theirs: &[String]) -> Vec<Region> {
    let pa = lcs_pairs(base, ours);
    let pb = lcs_pairs(base, theirs);
    let ca = diff_chunks(base, ours, &pa);
    let cb = diff_chunks(base, theirs, &pb);

    let mut out: Vec<Region> = Vec::new();
    let mut stable: Vec<String> = Vec::new();
    let (mut ai, mut bi) = (0usize, 0usize);
    let mut pos = 0usize; // base position already consumed

    while ai < ca.len() || bi < cb.len() {
        // seed with the chunk that starts first (ours wins ties)
        let seed_from_ours = match (ca.get(ai), cb.get(bi)) {
            (Some(a), Some(b)) => a.b_lo <= b.b_lo,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let (mut g_lo, mut g_hi) = if seed_from_ours {
            (ca[ai].b_lo, ca[ai].b_hi)
        } else {
            (cb[bi].b_lo, cb[bi].b_hi)
        };

        // transitive overlap closure
        let (mut a_used, mut b_used) = (0usize, 0usize);
        if seed_from_ours { a_used = 1 } else { b_used = 1 }
        loop {
            let mut grew = false;
            while ai + a_used < ca.len()
                && overlaps(g_lo, g_hi, ca[ai + a_used].b_lo, ca[ai + a_used].b_hi)
            {
                g_lo = g_lo.min(ca[ai + a_used].b_lo);
                g_hi = g_hi.max(ca[ai + a_used].b_hi);
                a_used += 1;
                grew = true;
            }
            while bi + b_used < cb.len()
                && overlaps(g_lo, g_hi, cb[bi + b_used].b_lo, cb[bi + b_used].b_hi)
            {
                g_lo = g_lo.min(cb[bi + b_used].b_lo);
                g_hi = g_hi.max(cb[bi + b_used].b_hi);
                b_used += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }

        // emit the untouched base lines before the group
        stable.extend(base[pos..g_lo].iter().cloned());

        if b_used == 0 {
            // only ours changed: apply its chunks in order
            for c in &ca[ai..ai + a_used] {
                stable.extend(ours[c.o_lo..c.o_hi].iter().cloned());
            }
        } else if a_used == 0 {
            // only theirs changed
            for c in &cb[bi..bi + b_used] {
                stable.extend(theirs[c.o_lo..c.o_hi].iter().cloned());
            }
        } else {
            // both sides changed the same region
            let o_from = extent_from(&pa, g_lo);
            let o_to = extent_to(&pa, g_hi, ours.len());
            let t_from = extent_from(&pb, g_lo);
            let t_to = extent_to(&pb, g_hi, theirs.len());
            if ours[o_from..o_to] == theirs[t_from..t_to] {
                // identical change on both sides
                stable.extend(ours[o_from..o_to].iter().cloned());
            } else {
                if !stable.is_empty() {
                    out.push(Region::Stable(std::mem::take(&mut stable)));
                }
                out.push(Region::Conflict {
                    ours: ours[o_from..o_to].to_vec(),
                    base: base[g_lo..g_hi].to_vec(),
                    theirs: theirs[t_from..t_to].to_vec(),
                });
            }
        }

        pos = g_hi;
        ai += a_used;
        bi += b_used;
    }
    stable.extend(base[pos..].iter().cloned());
    if !stable.is_empty() {
        out.push(Region::Stable(stable));
    }
    out
}

pub struct MergeOutcome {
    pub content: String,
    pub has_conflict: bool,
}

/// Render regions back to text; conflicts become diff3-style markers.
pub fn render_regions(regions: &[Region]) -> MergeOutcome {
    let mut buf = String::new();
    let mut has_conflict = false;
    for r in regions {
        match r {
            Region::Stable(lines) => {
                for l in lines {
                    let _ = writeln!(buf, "{l}");
                }
            }
            Region::Conflict { ours, base, theirs } => {
                has_conflict = true;
                buf.push_str("<<<<<<< ours\n");
                for l in ours {
                    let _ = writeln!(buf, "{l}");
                }
                buf.push_str("||||||| base\n");
                for l in base {
                    let _ = writeln!(buf, "{l}");
                }
                buf.push_str("=======\n");
                for l in theirs {
                    let _ = writeln!(buf, "{l}");
                }
                buf.push_str(">>>>>>> theirs\n");
            }
        }
    }
    MergeOutcome { content: buf, has_conflict }
}

/// Three-way merge of one file. `ours` is the target-branch side,
/// `theirs` is the source-branch side.
pub fn merge_file(base: &str, ours: &str, theirs: &str) -> MergeOutcome {
    if ours == theirs {
        return MergeOutcome { content: ours.to_string(), has_conflict: false };
    }
    if ours == base {
        return MergeOutcome { content: theirs.to_string(), has_conflict: false };
    }
    if theirs == base {
        return MergeOutcome { content: ours.to_string(), has_conflict: false };
    }
    let b = split_lines(base);
    let o = split_lines(ours);
    let t = split_lines(theirs);
    render_regions(&diff3(&b, &o, &t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(lines: &[&str]) -> String {
        lines.iter().map(|l| format!("{l}\n")).collect()
    }

    #[test]
    fn both_unchanged() {
        let m = merge_file("a\nb\nc\n", "a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(m.content, "a\nb\nc\n");
        assert!(!m.has_conflict);
    }

    #[test]
    fn one_side_changes_head_other_changes_tail() {
        let base = s(&["a", "b", "c"]);
        let ours = s(&["A", "b", "c"]);
        let theirs = s(&["a", "b", "C"]);
        let m = merge_file(&base, &ours, &theirs);
        assert!(!m.has_conflict, "{}", m.content);
        assert_eq!(m.content, s(&["A", "b", "C"]));
    }

    #[test]
    fn only_one_side_edits_middle() {
        let base = s(&["a", "b", "c"]);
        let ours = s(&["a", "B2", "c"]);
        let theirs = base.clone();
        let m = merge_file(&base, &ours, &theirs);
        assert!(!m.has_conflict);
        assert_eq!(m.content, ours);
    }

    #[test]
    fn conflicting_edit() {
        let base = s(&["a", "b", "c"]);
        let ours = s(&["a", "OURS", "c"]);
        let theirs = s(&["a", "THEIRS", "c"]);
        let m = merge_file(&base, &ours, &theirs);
        assert!(m.has_conflict);
        assert!(m.content.contains("<<<<<<< ours"));
        assert!(m.content.contains(">>>>>>> theirs"));
    }

    #[test]
    fn same_change_both_sides() {
        let base = s(&["a", "b", "c"]);
        let both = s(&["a", "X", "c"]);
        let m = merge_file(&base, &both, &both);
        assert!(!m.has_conflict);
        assert_eq!(m.content, both);
    }

    #[test]
    fn deletion_vs_edit_of_later_lines() {
        let base = s(&["a", "b", "c", "d"]);
        let ours = s(&["a", "d"]); // deleted b,c
        let theirs = s(&["a", "b", "c", "D"]); // edited d
        let m = merge_file(&base, &ours, &theirs);
        assert!(!m.has_conflict, "{}", m.content);
        assert_eq!(m.content, s(&["a", "D"]));
    }

    #[test]
    fn insertion_at_different_points() {
        let base = s(&["a", "c"]);
        let ours = s(&["a", "b", "c"]); // insert b before c
        let theirs = s(&["a", "c", "d"]); // append d
        let m = merge_file(&base, &ours, &theirs);
        assert!(!m.has_conflict, "{}", m.content);
        assert_eq!(m.content, s(&["a", "b", "c", "d"]));
    }

    #[test]
    fn different_insertions_at_same_point_conflict() {
        let base = s(&["a", "c"]);
        let ours = s(&["a", "B1", "c"]);
        let theirs = s(&["a", "B2", "c"]);
        let m = merge_file(&base, &ours, &theirs);
        assert!(m.has_conflict, "{}", m.content);
    }

    #[test]
    fn identical_insertions_at_same_point_resolve() {
        let base = s(&["a", "c"]);
        let ours = s(&["a", "B", "c"]);
        let theirs = s(&["a", "B", "c"]);
        let m = merge_file(&base, &ours, &theirs);
        assert!(!m.has_conflict);
        assert_eq!(m.content, ours);
    }

    #[test]
    fn interleaved_non_overlapping_edits() {
        let base = s(&["l1", "l2", "l3", "l4", "l5", "l6"]);
        let ours = s(&["l1", "O2", "l3", "l4", "l5", "l6"]); // edit l2
        let theirs = s(&["l1", "l2", "l3", "l4", "T5", "l6"]); // edit l5
        let m = merge_file(&base, &ours, &theirs);
        assert!(!m.has_conflict, "{}", m.content);
        assert_eq!(m.content, s(&["l1", "O2", "l3", "l4", "T5", "l6"]));
    }

    #[test]
    fn one_side_deletes_other_edits_same_line_conflict() {
        let base = s(&["a", "b", "c"]);
        let ours = s(&["a", "c"]); // deleted b
        let theirs = s(&["a", "B!", "c"]); // edited b
        let m = merge_file(&base, &ours, &theirs);
        assert!(m.has_conflict, "{}", m.content);
    }

    #[test]
    fn addition_while_other_side_unchanged() {
        let base = "";
        let ours = "new content\n";
        let theirs = ""; // unchanged
        let m = merge_file(base, ours, theirs);
        assert!(!m.has_conflict);
        assert_eq!(m.content, ours);
    }
}
