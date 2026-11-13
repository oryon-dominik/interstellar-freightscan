//! Classify lines by a language's comment markers — every language but Python goes through here.
//!
//! The scanner does not read string literals: a block marker inside a string opens a block. That
//! trade keeps it one table per language; Python, where it matters most, has its own classifier.

use crate::languages::{Language, UNKNOWN};
use crate::types::Kind;

type Block = (&'static str, &'static str);

/// Split text into lines on `\n` alone, the way the Python tokenizer numbers them.
pub fn split(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    if text.is_empty() || text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Return the kind of every line of `text`.
pub fn kinds(text: &str, language: &Language) -> Vec<Kind> {
    kinds_of_lines(&split(text), language)
}

/// Return the kind of every line, the lines already split.
pub fn kinds_of_lines(lines: &[&str], language: &Language) -> Vec<Kind> {
    if *language == UNKNOWN || language.prose {
        let filled = if language.prose { Kind::Prose } else { Kind::Unclassified };
        return lines.iter().map(|line| if line.trim().is_empty() { Kind::Blank } else { filled }).collect();
    }
    let starts_with_any = |text: &str, markers: &[&str]| markers.iter().any(|marker| opens_with(text, marker));
    let mut depth = 0usize;
    let mut block: Option<Block> = None;
    let mut block_kind = Kind::Comment;
    let mut block_start = 0usize;
    let mut result = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        let kind = if depth > 0 {
            block_kind
        } else if stripped.is_empty() {
            Kind::Blank
        } else if starts_with_any(stripped, language.doc_lines) || starts_with_any(stripped, language.doc_blocks) {
            Kind::Docstring
        } else if starts_with_any(stripped, language.line_comments)
            || language.blocks.iter().any(|(opening, _)| stripped.starts_with(opening))
        {
            Kind::Comment
        } else {
            Kind::Code
        };
        result.push(kind);
        if language.blocks.is_empty() {
            continue;
        }
        let was_open = depth > 0;
        (depth, block) = follow_blocks(line, language, depth, block);
        if depth > 0 && !was_open {
            block_start = index;
            block_kind = if kind == Kind::Docstring { Kind::Docstring } else { Kind::Comment };
        } else if was_open && depth == 0 && is_help(&lines[block_start..=index], language) {
            result[block_start..=index].fill(Kind::Docstring);
        }
    }
    result
}

/// Tell whether `text` opens with `marker`; a marker that is a word, like `REM`, must end as one.
fn opens_with(text: &str, marker: &str) -> bool {
    let Some(rest) = text.strip_prefix(marker) else { return false };
    !marker.ends_with(char::is_alphanumeric) || !rest.starts_with(char::is_alphanumeric)
}

/// Tell whether a comment block names a help keyword after a dot, as PowerShell's help does.
fn is_help(lines: &[&str], language: &Language) -> bool {
    lines.iter().any(|line| {
        let Some(rest) = line.trim().strip_prefix('.') else { return false };
        let keyword = rest.split(' ').next().unwrap_or_default().to_ascii_uppercase();
        language.help_keywords.contains(&keyword.as_str())
    })
}

/// Jump from marker to marker through one line and return the open block and its depth after it.
fn follow_blocks(
    line: &str,
    language: &Language,
    mut depth: usize,
    mut block: Option<Block>,
) -> (usize, Option<Block>) {
    let find = |marker: &str, from: usize| line[from..].find(marker).map(|at| at + from);
    let mut position = 0;
    loop {
        if let (true, Some((opening, closing))) = (depth > 0, block) {
            let close_at = find(closing, position);
            let open_at = if language.nested { find(opening, position) } else { None };
            match (open_at, close_at) {
                (Some(open), close) if close.is_none_or(|close| open < close) => {
                    depth += 1;
                    position = open + opening.len();
                }
                (_, Some(close)) => {
                    depth -= 1;
                    position = close + closing.len();
                }
                _ => return (depth, block),
            }
            continue;
        }
        let Some((at, opened)) = language
            .blocks
            .iter()
            .filter_map(|candidate| find(candidate.0, position).map(|at| (at, *candidate)))
            .min_by_key(|(at, _)| *at)
        else {
            return (0, None);
        };
        let comment = language.line_comments.iter().filter_map(|marker| find(marker, position)).min();
        // A line comment before the block marker swallows it — unless the block starts with that marker (Lua's --[[).
        if comment.is_some_and(|comment| comment < at) {
            return (0, None);
        }
        depth = 1;
        block = Some(opened);
        position = at + opened.0.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages;

    static C_LIKE: Language = Language {
        name: "C-like",
        line_comments: &["//"],
        blocks: &[("/*", "*/")],
        nested: false,
        doc_lines: &[],
        doc_blocks: &[],
        help_keywords: &[],
        prose: false,
    };
    static NESTING: Language = Language { name: "C-like nesting", nested: true, ..C_LIKE };

    use Kind::*;

    #[test]
    fn a_line_starting_with_a_hash_is_a_comment() {
        for language in [&languages::TOML, &languages::YAML, &languages::JUSTFILE, &languages::ENV, &languages::SHELL] {
            let text = "# what follows\nkey = 1\n\n  # indented\n";
            assert_eq!(kinds(text, language), [Comment, Code, Blank, Comment], "{}", language.name);
        }
    }

    #[test]
    fn a_comment_after_code_leaves_the_line_code() {
        assert_eq!(kinds("key = 1  # why\n", &languages::TOML), [Code]);
    }

    #[test]
    fn an_unknown_language_counts_only_blank_and_unclassified() {
        let text = "plain\n\n# not a comment here\n";
        assert_eq!(kinds(text, &languages::UNKNOWN), [Unclassified, Blank, Unclassified]);
    }

    #[test]
    fn text_counts_as_prose() {
        assert_eq!(kinds("A fortune.\n\n%\n", &languages::TEXT), [Prose, Blank, Prose]);
    }

    #[test]
    fn every_line_of_a_block_comment_is_a_comment_blank_ones_included() {
        assert_eq!(kinds("/* one\n\n   two */\ncode();\n", &C_LIKE), [Comment, Comment, Comment, Code]);
    }

    #[test]
    fn a_block_opened_after_code_makes_the_following_lines_comments() {
        assert_eq!(kinds("code(); /* starts\nstill */\ncode();\n", &C_LIKE), [Code, Comment, Code]);
    }

    #[test]
    fn a_block_marker_inside_a_line_comment_opens_no_block() {
        assert_eq!(kinds("// see /* this\ncode();\n", &C_LIKE), [Comment, Code]);
    }

    #[test]
    fn without_nesting_the_first_closing_marker_ends_the_block() {
        assert_eq!(kinds("/* outer /* inner */\ncode();\n", &C_LIKE), [Comment, Code]);
    }

    #[test]
    fn with_nesting_only_the_outermost_closing_marker_ends_the_block() {
        assert_eq!(kinds("/* outer /* inner */\nstill outer */\ncode();\n", &NESTING), [Comment, Comment, Code]);
    }

    #[test]
    fn a_lua_block_comment_opens_with_its_line_marker() {
        assert_eq!(kinds("--[[ one\ntwo ]]\nprint(1)\n-- why\n", &languages::LUA), [Comment, Comment, Code, Comment]);
    }

    #[test]
    fn batch_files_comment_with_rem_and_double_colons() {
        assert_eq!(
            kinds("@echo off\nREM why\n:: how\nrem low\n", &languages::BATCH),
            [Code, Comment, Comment, Comment]
        );
    }

    #[test]
    fn a_comment_word_needs_its_word_boundary() {
        assert_eq!(kinds("remote.exe\nREM\nrem why\n", &languages::BATCH), [Code, Comment, Comment]);
    }

    #[test]
    fn rust_documentation_comments_are_docstrings() {
        let text = "//! Crate.\n/// Item.\n// plain\nfn main() {}\n";
        assert_eq!(kinds(text, &languages::RUST), [Docstring, Docstring, Comment, Code]);
    }

    #[test]
    fn a_rust_documentation_block_is_a_docstring() {
        let text = "/** Item.\n * More.\n */\n/* plain */\nfn main() {}\n";
        assert_eq!(kinds(text, &languages::RUST), [Docstring, Docstring, Docstring, Comment, Code]);
    }

    #[test]
    fn nested_rust_block_comments_end_at_the_outermost_marker() {
        let text = "/* outer\n/* inner */\nstill outer */\nfn main() {}\n";
        assert_eq!(kinds(text, &languages::RUST), [Comment, Comment, Comment, Code]);
    }

    #[test]
    fn powershell_comment_based_help_is_a_docstring() {
        let text = "<#\n.SYNOPSIS\n    Launch the probe.\n#>\n<# a plain block #>\nWrite-Output 'go'\n";
        assert_eq!(kinds(text, &languages::POWERSHELL), [Docstring, Docstring, Docstring, Docstring, Comment, Code]);
    }

    #[test]
    fn django_template_comments_are_comments() {
        let text = "{# one line #}\n{% comment %}\na longer note\n{% endcomment %}\n<p>{{ value }}</p>\n";
        assert_eq!(kinds(text, &languages::DJANGO_TEMPLATE), [Comment, Comment, Comment, Comment, Code]);
    }

    #[test]
    fn split_reads_windows_line_ends_and_adds_no_line_for_the_final_newline() {
        assert_eq!(split("one\r\ntwo\r\n"), ["one", "two"]);
    }

    #[test]
    fn split_keeps_a_last_line_without_newline() {
        assert_eq!(split("one\ntwo"), ["one", "two"]);
    }

    #[test]
    fn split_of_nothing_is_no_line() {
        assert!(split("").is_empty());
    }
}
