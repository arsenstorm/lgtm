//! Word-level segments for a deletion paired with the addition that replaced it.

use crate::{change_run, Line, LineKind, Segment};

/// Lines longer than this skip the intra-line word diff.
const MAX_INTRALINE_LEN: usize = 1000;

/// Pairs each run of deletions with the additions that follow it and gives the
/// paired lines word-level segments.
pub fn mark_intraline(lines: &mut [Line]) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != LineKind::Deletion {
            i += 1;
            continue;
        }
        let (add_start, end) = change_run(lines, i);
        let pairs = (add_start - i).min(end - add_start);
        for k in 0..pairs {
            let (dels, adds) = lines.split_at_mut(add_start);
            mark_pair(&mut dels[i + k], &mut adds[k]);
        }
        i = end;
    }
}

fn mark_pair(del: &mut Line, add: &mut Line) {
    if del.text.len() > MAX_INTRALINE_LEN || add.text.len() > MAX_INTRALINE_LEN {
        return;
    }
    let old_tokens = tokenize(&del.text);
    let new_tokens = tokenize(&add.text);
    let (old_kept, new_kept) = lcs_kept(&old_tokens, &new_tokens);
    del.segments = segments(&old_tokens, &old_kept);
    add.segments = segments(&new_tokens, &new_kept);
}

/// Splits on word boundaries and whitespace, keeping separators as tokens.
fn tokenize(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let class = char_class(c);
        let mut end = start + c.len_utf8();
        if class != CharClass::Other {
            while let Some(&(i, next)) = chars.peek() {
                if char_class(next) != class {
                    break;
                }
                end = i + next.len_utf8();
                chars.next();
            }
        }
        tokens.push(&text[start..end]);
    }
    tokens
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Space,
    Other,
}

fn char_class(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Space
    } else {
        CharClass::Other
    }
}

/// Longest common subsequence over tokens; the flags mark the tokens inside it.
fn lcs_kept(old: &[&str], new: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (old.len(), new.len());
    let table = lcs_table(old, new);
    let at = |i: usize, j: usize| i * (m + 1) + j;
    let mut old_kept = vec![false; n];
    let mut new_kept = vec![false; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            old_kept[i] = true;
            new_kept[j] = true;
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (old_kept, new_kept)
}

/// Suffix LCS lengths, `(n + 1) * (m + 1)` cells with `[i][j]` at `i * (m + 1) + j`.
fn lcs_table(old: &[&str], new: &[&str]) -> Vec<u32> {
    let (n, m) = (old.len(), new.len());
    let mut table = vec![0_u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let cell = match old[i] == new[j] {
                true => table[at(i + 1, j + 1)] + 1,
                false => table[at(i + 1, j)].max(table[at(i, j + 1)]),
            };
            table[at(i, j)] = cell;
        }
    }
    table
}

/// Merges neighbouring tokens that share a changed flag into segments.
fn segments(tokens: &[&str], kept: &[bool]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for (token, keep) in tokens.iter().zip(kept) {
        let changed = !keep;
        match out.last_mut() {
            Some(last) if last.changed == changed => last.text.push_str(token),
            _ => out.push(Segment {
                text: (*token).to_string(),
                changed,
            }),
        }
    }
    if out.is_empty() {
        out.push(Segment {
            text: String::new(),
            changed: false,
        });
    }
    out
}
