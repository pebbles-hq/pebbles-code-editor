//! Collaboration primitives: a minimal text [`Edit`] delta plus the pure functions to derive
//! one from an old→new text pair and to map a byte position across it. These make the editor
//! OT/CRDT-ready: local edits are reported as `Edit`s (the outbound op stream), and an inbound
//! remote change (the `code` signal set from outside) is applied with the local caret remapped
//! through the same delta so a collaborator's edit never yanks your cursor.

/// A single contiguous text change: replace `from..to` (bytes, in the OLD text) with `insert`.
/// A pure insert has `from == to`; a pure delete has an empty `insert`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Start byte of the replaced range in the OLD text.
    pub from: usize,
    /// End byte (exclusive) of the replaced range in the OLD text.
    pub to: usize,
    /// The text inserted in place of `from..to`.
    pub insert: String,
}

/// The minimal single-range delta turning `old` into `new` (common prefix/suffix trimmed),
/// or `None` if they're identical. Char-boundary safe (handles multi-byte / CJK).
pub(crate) fn diff(old: &str, new: &str) -> Option<Edit> {
    if old == new {
        return None;
    }
    // Byte-level prefix/suffix (chunked slice compares — this runs on every keystroke over
    // the whole document), then snapped back to char boundaries in BOTH texts. Equal bytes
    // mean equal chars, so this is the same edit as a char-by-char trim.
    let (ob, nb) = (old.as_bytes(), new.as_bytes());
    let mut p = crate::lines::common_prefix(ob, nb);
    while !(old.is_char_boundary(p) && new.is_char_boundary(p)) {
        p -= 1;
    }
    let mut s = crate::lines::common_suffix(ob, nb, ob.len().min(nb.len()) - p);
    while !(old.is_char_boundary(old.len() - s) && new.is_char_boundary(new.len() - s)) {
        s -= 1;
    }
    Some(Edit {
        from: p,
        to: old.len() - s,
        insert: new[p..new.len() - s].to_string(),
    })
}

/// Map a byte position from the OLD text to the NEW text across `e`.
pub(crate) fn map_pos(pos: usize, e: &Edit) -> usize {
    if pos <= e.from {
        pos
    } else if pos >= e.to {
        pos - (e.to - e.from) + e.insert.len()
    } else {
        // Inside the replaced range collapses to the end of the inserted text.
        e.from + e.insert.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original char-by-char trim, kept as the reference for the byte-level `diff`.
    fn diff_chars(old: &str, new: &str) -> Option<Edit> {
        if old == new {
            return None;
        }
        let o: Vec<(usize, char)> = old.char_indices().collect();
        let n: Vec<(usize, char)> = new.char_indices().collect();
        let mut i = 0;
        while i < o.len() && i < n.len() && o[i].1 == n[i].1 {
            i += 1;
        }
        let mut j = 0;
        while j < (o.len() - i) && j < (n.len() - i) && o[o.len() - 1 - j].1 == n[n.len() - 1 - j].1
        {
            j += 1;
        }
        let byte = |v: &[(usize, char)], idx: usize, total: usize| {
            v.get(idx).map(|c| c.0).unwrap_or(total)
        };
        let ins_start = byte(&n, i, new.len());
        let ins_end = byte(&n, n.len() - j, new.len());
        Some(Edit {
            from: byte(&o, i, old.len()),
            to: byte(&o, o.len() - j, old.len()),
            insert: new[ins_start..ins_end].to_string(),
        })
    }

    #[test]
    fn diff_matches_char_reference_for_random_edits() {
        let alphabet = ["a", "b", "é", "è", "日", "本", "\n", " ", "🦀"];
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        let mut rnd = |n: usize| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % n as u64) as usize
        };
        for _ in 0..3000 {
            let mk = |r: &mut dyn FnMut(usize) -> usize| -> String {
                (0..r(400)).map(|_| alphabet[r(alphabet.len())]).collect()
            };
            let base = mk(&mut rnd);
            let mut a = base.clone();
            let mut b = base;
            // Mutate each side a little so they share long prefixes/suffixes.
            for t in [&mut a, &mut b] {
                if rnd(2) == 0 {
                    let cut: Vec<char> = t.chars().collect();
                    let at = rnd(cut.len() + 1);
                    let to = (at + rnd(4)).min(cut.len());
                    let ins = alphabet[rnd(alphabet.len())];
                    *t = cut[..at].iter().collect::<String>()
                        + ins
                        + &cut[to..].iter().collect::<String>();
                }
            }
            assert_eq!(diff(&a, &b), diff_chars(&a, &b), "{a:?} -> {b:?}");
        }
        for (a, b) in [
            ("aéa", "aèa"),
            ("é", "è"),
            ("", "x"),
            ("x", ""),
            ("ab", "aab"),
            ("🦀", "日"),
        ] {
            assert_eq!(diff(a, b), diff_chars(a, b), "{a:?} -> {b:?}");
        }
    }

    #[test]
    fn diff_insert_in_middle() {
        assert_eq!(
            diff("abcf", "abcdef"),
            Some(Edit {
                from: 3,
                to: 3,
                insert: "de".into()
            })
        );
    }

    #[test]
    fn diff_delete() {
        assert_eq!(
            diff("hello world", "hello"),
            Some(Edit {
                from: 5,
                to: 11,
                insert: String::new()
            })
        );
    }

    #[test]
    fn diff_replace_and_none() {
        assert_eq!(
            diff("cat", "cot"),
            Some(Edit {
                from: 1,
                to: 2,
                insert: "o".into()
            })
        );
        assert_eq!(diff("same", "same"), None);
    }

    #[test]
    fn diff_is_char_boundary_safe() {
        // Inserting after a multi-byte char must land on a boundary.
        let e = diff("café", "café!").unwrap();
        assert_eq!(
            e,
            Edit {
                from: 5,
                to: 5,
                insert: "!".into()
            }
        );
    }

    #[test]
    fn map_pos_shifts_after_insert() {
        let e = Edit {
            from: 3,
            to: 3,
            insert: "de".into(),
        }; // "abcf" -> "abcdef"
        assert_eq!(map_pos(2, &e), 2); // before the edit — unchanged
        assert_eq!(map_pos(3, &e), 3); // at the insert point
        assert_eq!(map_pos(4, &e), 6); // after — shifted by +2
    }

    #[test]
    fn map_pos_across_delete() {
        let e = Edit {
            from: 5,
            to: 11,
            insert: String::new(),
        }; // delete "  world"
        assert_eq!(map_pos(11, &e), 5);
        assert_eq!(map_pos(8, &e), 5); // inside the deletion collapses to `from`
    }
}
