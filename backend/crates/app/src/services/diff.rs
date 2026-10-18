//! Line-based unified diff (LCS) used by the document version history UI.
//!
//! Output mirrors `git diff --unified=N`: hunk headers plus `-`/`+`/` ` lines.

/// LCS alignment between two line slices as monotone (a_idx, b_idx) pairs.
fn lcs_pairs(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    /// keep line (a_idx)
    Keep(usize),
    /// delete line (a_idx)
    Del(usize),
    /// insert line (b_idx)
    Ins(usize),
}

fn edit_script<'a>(a: &[&'a str], b: &[&'a str], pairs: &[(usize, usize)]) -> Vec<Op> {
    let mut ops = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    for &(ai, bi) in pairs {
        while i < ai {
            ops.push(Op::Del(i));
            i += 1;
        }
        while j < bi {
            ops.push(Op::Ins(j));
            j += 1;
        }
        ops.push(Op::Keep(ai));
        i = ai + 1;
        j = bi + 1;
    }
    while i < a.len() {
        ops.push(Op::Del(i));
        i += 1;
    }
    while j < b.len() {
        ops.push(Op::Ins(j));
        j += 1;
    }
    ops
}

/// Split into lines, preserving a trailing newline's effect by dropping the
/// final empty element (same convention as `merge.rs`).
fn split_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return vec![];
    }
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

struct Hunk {
    a_start: usize,
    b_start: usize,
    a_len: usize,
    b_len: usize,
    lines: Vec<(char, String)>,
}

fn render_hunk(
    ops: &[Op],
    a: &[&str],
    b: &[&str],
    positions: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Hunk {
    let (a_start, b_start) = positions[start];
    let mut lines = Vec::new();
    let mut a_len = 0usize;
    let mut b_len = 0usize;
    for op in &ops[start..end] {
        match *op {
            Op::Keep(i) => {
                lines.push((' ', a[i].to_string()));
                a_len += 1;
                b_len += 1;
            }
            Op::Del(i) => {
                lines.push(('-', a[i].to_string()));
                a_len += 1;
            }
            Op::Ins(j) => {
                lines.push(('+', b[j].to_string()));
                b_len += 1;
            }
        }
    }
    Hunk {
        a_start,
        b_start,
        a_len,
        b_len,
        lines,
    }
}

/// Unified diff of two texts with `context` unchanged lines around changes.
/// Returns an empty string when the inputs are identical.
pub fn unified(old: &str, new: &str, context: usize) -> String {
    if old == new {
        return String::new();
    }
    let a = split_lines(old);
    let b = split_lines(new);
    let pairs = lcs_pairs(&a, &b);
    let ops = edit_script(&a, &b, &pairs);

    // (a_idx, b_idx) position at the start of every op
    let mut positions: Vec<(usize, usize)> = Vec::with_capacity(ops.len());
    let (mut a_pos, mut b_pos) = (0usize, 0usize);
    for op in &ops {
        positions.push((a_pos, b_pos));
        match *op {
            Op::Keep(_) => {
                a_pos += 1;
                b_pos += 1;
            }
            Op::Del(_) => a_pos += 1,
            Op::Ins(_) => b_pos += 1,
        }
    }

    // split into clusters of changes separated by more than 2*context keeps
    let mut clusters: Vec<(usize, usize)> = Vec::new(); // [first_change, last_change] inclusive
    for (i, op) in ops.iter().enumerate() {
        if matches!(op, Op::Keep(_)) {
            continue;
        }
        match clusters.last_mut() {
            Some((_, last)) => {
                let gap_keeps = ops[*last + 1..i]
                    .iter()
                    .filter(|o| matches!(o, Op::Keep(_)))
                    .count();
                if gap_keeps <= context * 2 {
                    *last = i;
                } else {
                    clusters.push((i, i));
                }
            }
            None => clusters.push((i, i)),
        }
    }
    if clusters.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for (first, last) in clusters {
        let start = first.saturating_sub(context);
        let end = (last + 1 + context).min(ops.len());
        let h = render_hunk(&ops, &a, &b, &positions, start, end);
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            h.a_start + 1,
            h.a_len,
            h.b_start + 1,
            h.b_len
        ));
        for (kind, text) in h.lines {
            out.push(kind);
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_empty() {
        assert_eq!(unified("a\nb\n", "a\nb\n", 3), "");
    }

    #[test]
    fn single_line_change() {
        let d = unified("a\nb\nc\n", "a\nB\nc\n", 3);
        assert!(d.contains("@@ -1,3 +1,3 @@"), "{d}");
        assert!(d.contains("-b"), "{d}");
        assert!(d.contains("+B"), "{d}");
    }

    #[test]
    fn added_trailing_line() {
        let d = unified("a\n", "a\nb\n", 3);
        assert!(d.contains("+b"), "{d}");
        assert!(
            !d.lines().any(|l| l.starts_with('-')),
            "no deletion lines expected: {d}"
        );
    }

    #[test]
    fn removed_line() {
        let d = unified("a\nb\nc\n", "a\nc\n", 3);
        assert!(d.contains("-b"), "{d}");
    }

    #[test]
    fn context_is_limited() {
        let old: String = (1..=20).map(|i| format!("l{i}\n")).collect();
        let mut lines: Vec<String> = (1..=20).map(|i| format!("l{i}\n")).collect();
        lines[9] = "CHANGED\n".into();
        let new: String = lines.concat();
        let d = unified(&old, &new, 2);
        assert!(d.contains("-l10"), "{d}");
        assert!(d.contains("+CHANGED"), "{d}");
        // l1 / l20 sit far outside the 2-line context window
        assert!(!d.contains(" l1\n"), "{d}");
        assert!(!d.contains(" l20"), "{d}");
    }
}
