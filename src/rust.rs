//! Find the lines of a Rust file that only exist in test builds.
//!
//! Knows just enough Rust to find where an item ends: comments, strings, raw strings and
//! character literals hide their brackets from the count.

const GATE: &str = "#[cfg(test)]";

/// Mark every line an item behind `#[cfg(test)]` takes, the attribute's own first.
pub fn test_lines(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut lines = vec![false; text.matches('\n').count() + 1];
    let mut line = 0;
    let mut position = 0;
    let mut depth = 0usize;
    // The depth the open item stands at, and its first line.
    let mut gate: Option<(usize, usize)> = None;
    while position < bytes.len() {
        let byte = bytes[position];
        let end = match (byte, bytes.get(position + 1)) {
            (b'/', Some(b'/')) => {
                bytes[position..].iter().position(|next| *next == b'\n').map_or(bytes.len(), |at| position + at)
            }
            (b'/', Some(b'*')) => block_comment_end(bytes, position),
            (b'"', _) => string_end(bytes, position + 1),
            (b'\'', _) => character_end(text, position),
            (b'#', _) if text[position..].starts_with(GATE) => {
                gate.get_or_insert((depth, line));
                position + GATE.len()
            }
            (b'r' | b'b' | b'c', _) => raw_string_end(bytes, position).unwrap_or(position + 1),
            (b'(' | b'[' | b'{', _) => {
                depth += 1;
                position + 1
            }
            (b')' | b']' | b'}', _) => {
                depth = depth.saturating_sub(1);
                position + 1
            }
            _ => position + 1,
        };
        if let Some((base, first)) = gate
            && matches!(byte, b'}' | b';')
            && depth == base
        {
            lines[first..=line].fill(true);
            gate = None;
        }
        let end = end.min(bytes.len());
        line += bytes[position..end].iter().filter(|next| **next == b'\n').count();
        position = end;
    }
    if let Some((_, first)) = gate {
        lines[first..=line].fill(true);
    }
    lines
}

/// The end of a block comment, which nests in Rust.
fn block_comment_end(bytes: &[u8], mut position: usize) -> usize {
    let mut depth = 0usize;
    while position + 1 < bytes.len() {
        match &bytes[position..position + 2] {
            b"/*" => {
                depth += 1;
                position += 2;
            }
            b"*/" => {
                depth -= 1;
                position += 2;
                if depth == 0 {
                    return position;
                }
            }
            _ => position += 1,
        }
    }
    bytes.len()
}

/// The end of a string, from the first byte behind its opening quote.
fn string_end(bytes: &[u8], mut position: usize) -> usize {
    while position < bytes.len() {
        match bytes[position] {
            b'\\' => position += 2,
            b'"' => return position + 1,
            _ => position += 1,
        }
    }
    bytes.len()
}

/// The end of a character literal; a lifetime has none, so only its quote is passed.
fn character_end(text: &str, position: usize) -> usize {
    let rest = &text[position + 1..];
    let Some(first) = rest.chars().next() else { return position + 1 };
    if first == '\\' {
        let escaped = 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
        return rest[escaped..].find('\'').map_or(text.len(), |at| position + 1 + escaped + at + 1);
    }
    let after = first.len_utf8();
    if rest[after..].starts_with('\'') { position + 1 + after + 1 } else { position + 1 }
}

/// The end of a raw string starting here — `r"`, `r#"`, `br#"` — or None when none starts here.
fn raw_string_end(bytes: &[u8], position: usize) -> Option<usize> {
    let start = position + usize::from(matches!(bytes[position], b'b' | b'c'));
    if bytes.get(start) != Some(&b'r') {
        return None;
    }
    let hashes = bytes[start + 1..].iter().take_while(|next| **next == b'#').count();
    let opening = start + 1 + hashes;
    if bytes.get(opening) != Some(&b'"') {
        return None;
    }
    let closing: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
    let body = opening + 1;
    Some(
        bytes[body..]
            .windows(closing.len())
            .position(|next| next == closing)
            .map_or(bytes.len(), |at| body + at + closing.len()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(text: &str) -> Vec<usize> {
        test_lines(text).iter().enumerate().filter(|(_, test)| **test).map(|(line, _)| line + 1).collect()
    }

    #[test]
    fn a_gated_module_is_test_code_to_its_closing_brace() {
        assert_eq!(marked("fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() {}\n}\nfn c() {}\n"), [2, 3, 4, 5]);
    }

    #[test]
    fn a_gated_item_without_a_body_ends_at_its_semicolon() {
        assert_eq!(marked("#[cfg(test)]\nuse std::fs;\nfn a() {}\n"), [1, 2]);
        assert_eq!(marked("#[cfg(test)]\nconst SIZE: [u8; 4] = [0; 4];\nfn a() {}\n"), [1, 2]);
    }

    #[test]
    fn brackets_in_strings_comments_and_characters_do_not_count() {
        let text = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    const A: &str = \"}\";\n",
            "    const B: char = '}';\n",
            "    const C: &str = r#\"}\"#;\n",
            "    // }\n",
            "    /* } /* } */ } */\n",
            "    fn d<'a>(e: &'a str) {}\n",
            "}\n",
            "fn f() {}\n",
        );
        assert_eq!(marked(text), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn a_gate_quoted_in_a_string_or_comment_opens_nothing() {
        assert!(marked("// #[cfg(test)]\nconst A: &str = \"#[cfg(test)]\";\nfn a() {}\n").is_empty());
    }

    #[test]
    fn a_gated_item_that_never_ends_runs_to_the_end_of_the_file() {
        assert_eq!(marked("#[cfg(test)]\nmod tests {\n"), [1, 2, 3]);
    }
}
