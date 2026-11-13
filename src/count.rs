//! Count one file, and gather the files of one directory into a scan.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::types::{Cell, Counted, Exclusion, FileCount, Kind, Role, Scan};
use crate::{exclusions, languages, python, roles, rust, scanner, sections};

/// Count the lines of one file, or say why it is left out.
pub fn count_file(path: &str, data: &[u8]) -> Counted {
    if let Some(reason) = exclusions::exclusion(path, data) {
        return Counted::Excluded(reason);
    }
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let Ok(text) = std::str::from_utf8(data) else { return Counted::Excluded(Exclusion::Undecodable) };
    let language = languages::language_of(path);
    let role = roles::role(path, language);
    let mut unparsed = false;
    let segments = if *language == languages::PYTHON {
        let kinds = python::kinds(text);
        unparsed = kinds.is_none();
        vec![(language, kinds.unwrap_or_else(|| scanner::kinds(text, language)))]
    } else {
        sections::segments(text, language)
    };
    // Rust keeps its tests inside its source files.
    let inline = if *language == languages::RUST && role == Role::Source { rust::test_lines(text) } else { Vec::new() };
    let mut tally: BTreeMap<(bool, &'static str, Role, Kind), u64> = BTreeMap::new();
    let lines = segments.into_iter().flat_map(|(embedded, kinds)| kinds.into_iter().map(move |kind| (embedded, kind)));
    for (index, (embedded, mut kind)) in lines.enumerate() {
        // A data file holds records, not code: every line but a blank one is data.
        if role == Role::Data && kind != Kind::Blank {
            kind = Kind::Data;
        }
        let role = if inline.get(index).copied().unwrap_or(false) { Role::Test } else { role };
        *tally.entry((embedded.name != language.name, embedded.name, role, kind)).or_default() += 1;
    }
    let cells =
        tally.into_iter().map(|((_, name, role, kind), lines)| Cell { language: name, role, kind, lines }).collect();
    Counted::File(FileCount { path: path.to_owned(), cells, unparsed })
}

/// Gather counted files and exclusions of one directory into a scan.
pub fn assemble(root: PathBuf, results: impl IntoIterator<Item = Counted>) -> Scan {
    let mut files = Vec::new();
    let mut excluded = BTreeMap::new();
    for result in results {
        match result {
            Counted::File(file) => files.push(file),
            Counted::Excluded(reason) => *excluded.entry(reason).or_default() += 1,
        }
    }
    let mut scan = Scan { name: String::new(), root, files, excluded };
    scan.name = scan.root_text();
    scan
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn file(path: &str, data: &[u8]) -> FileCount {
        match count_file(path, data) {
            Counted::File(file) => file,
            Counted::Excluded(reason) => panic!("{path} excluded as {reason}"),
        }
    }

    /// The cells of a file as (language, kind, lines), the role left out.
    fn cells(path: &str, text: &str) -> BTreeSet<(&'static str, Kind, u64)> {
        file(path, text.as_bytes()).cells.into_iter().map(|cell| (cell.language, cell.kind, cell.lines)).collect()
    }

    fn cell(language: &'static str, role: Role, kind: Kind, lines: u64) -> Cell {
        Cell { language, role, kind, lines }
    }

    #[test]
    fn a_binary_file_is_excluded() {
        assert_eq!(
            count_file("locale/de/django.mo", b"\xde\x12\x04\x95\x00\x00"),
            Counted::Excluded(Exclusion::Binary)
        );
    }

    #[test]
    fn a_lock_file_is_excluded() {
        for name in ["uv.lock", "Cargo.lock", "package-lock.json", "pnpm-lock.yaml"] {
            assert_eq!(count_file(name, b"version = 1\n"), Counted::Excluded(Exclusion::Lock), "{name}");
        }
    }

    #[test]
    fn a_minified_asset_is_excluded() {
        for name in ["assets/htmx.min.js", "assets/theme.min.css"] {
            assert_eq!(count_file(name, b"x\n"), Counted::Excluded(Exclusion::Minified), "{name}");
        }
    }

    #[test]
    fn a_file_that_is_not_utf8_is_excluded() {
        assert_eq!(count_file("notes.txt", b"Gr\xfc\xdfe\n"), Counted::Excluded(Exclusion::Undecodable));
    }

    #[test]
    fn every_line_lands_in_exactly_one_cell() {
        let counted = file("src/probe.py", b"\"\"\"Doc.\"\"\"\n\n# why\nvalue = 1\n");
        assert_eq!(
            counted.cells,
            [
                cell("Python", Role::Source, Kind::Code, 1),
                cell("Python", Role::Source, Kind::Docstring, 1),
                cell("Python", Role::Source, Kind::Comment, 1),
                cell("Python", Role::Source, Kind::Blank, 1),
            ]
        );
    }

    #[test]
    fn every_line_of_a_data_file_but_the_blank_ones_is_data() {
        assert_eq!(
            file("records/run.json", b"{\n  \"lines\": 1\n\n}\n").cells,
            [cell("JSON", Role::Data, Kind::Data, 3), cell("JSON", Role::Data, Kind::Blank, 1)]
        );
        assert_eq!(file("logs/run.log", b"started\n").cells, [cell("Log", Role::Data, Kind::Data, 1)]);
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_line() {
        assert_eq!(file("probe.py", b"\xef\xbb\xbfvalue = 1\n").cells, [cell("Python", Role::Source, Kind::Code, 1)]);
    }

    #[test]
    fn unparsable_python_is_counted_by_comment_markers_and_marked() {
        let counted = file("broken.py", b"# why\ndef broken(:\n");
        assert!(counted.unparsed);
        assert_eq!(
            counted.cells,
            [cell("Python", Role::Source, Kind::Code, 1), cell("Python", Role::Source, Kind::Comment, 1)]
        );
    }

    #[test]
    fn assemble_keeps_the_files_and_counts_the_exclusions_per_reason() {
        let probe = count_file("probe.py", b"value = 1\n");
        let excluded = Counted::Excluded(Exclusion::Binary);
        let scan = assemble(PathBuf::from("repository"), [probe.clone(), excluded.clone(), excluded]);
        assert_eq!(scan.files.len(), 1);
        assert_eq!(scan.excluded, BTreeMap::from([(Exclusion::Binary, 2)]));
    }

    #[test]
    fn markdown_outside_fences_is_prose() {
        let expected = BTreeSet::from([
            ("Markdown", Kind::Prose, 2),
            ("Markdown", Kind::Blank, 1),
            ("Markdown", Kind::Comment, 1),
        ]);
        assert_eq!(cells("README.md", "# Title\n\nSome words.\n<!-- a note -->\n"), expected);
    }

    #[test]
    fn a_fenced_block_counts_under_the_language_it_names() {
        let text = "Run it:\n\n```python\n# set up\nvalue = 1\n```\n";
        let expected = BTreeSet::from([
            ("Markdown", Kind::Prose, 3),
            ("Markdown", Kind::Blank, 1),
            ("Python", Kind::Comment, 1),
            ("Python", Kind::Code, 1),
        ]);
        assert_eq!(cells("README.md", text), expected);
    }

    #[test]
    fn a_fence_without_a_known_language_stays_markdown() {
        let expected = BTreeSet::from([("Markdown", Kind::Prose, 2), ("Markdown", Kind::Unclassified, 1)]);
        assert_eq!(cells("README.md", "```\nsome output\n```\n"), expected);
    }

    #[test]
    fn a_fence_naming_a_file_counts_under_that_file_s_language() {
        let expected = BTreeSet::from([("Markdown", Kind::Prose, 2), ("Django template", Kind::Code, 1)]);
        assert_eq!(cells("README.md", "```base.djt.html\n<p>hello</p>\n```\n"), expected);
    }

    #[test]
    fn a_fence_of_markdown_is_prose() {
        let expected = BTreeSet::from([("Markdown", Kind::Prose, 3)]);
        assert_eq!(cells("README.md", "```markdown\n# Title\n```\n"), expected);
    }

    #[test]
    fn a_fence_named_text_is_prose() {
        let expected = BTreeSet::from([("Markdown", Kind::Prose, 2), ("Text", Kind::Prose, 1)]);
        assert_eq!(cells("README.md", "```text\nsome words\n```\n"), expected);
    }

    fn notebook(cells: serde_json::Value) -> String {
        serde_json::json!({"cells": cells, "metadata": {"kernelspec": {"language": "python"}}, "nbformat": 4})
            .to_string()
    }

    #[test]
    fn code_cells_count_under_the_kernel_language_and_markdown_cells_as_prose() {
        let text = notebook(serde_json::json!([
            {"cell_type": "markdown", "source": ["# Analysis\n", "\n", "Why we look."]},
            {"cell_type": "code", "source": ["# load\n", "value = 1"], "outputs": []},
        ]));
        let expected = BTreeSet::from([
            ("Jupyter Notebook", Kind::Prose, 2),
            ("Jupyter Notebook", Kind::Blank, 1),
            ("Python", Kind::Comment, 1),
            ("Python", Kind::Code, 1),
        ]);
        assert_eq!(cells("analysis.ipynb", &text), expected);
    }

    #[test]
    fn outputs_and_metadata_are_not_counted() {
        let text = notebook(serde_json::json!([{
            "cell_type": "code",
            "source": "value = 1",
            "outputs": [{"output_type": "stream", "text": ["1\n", "2\n", "3\n"]}],
            "metadata": {"tags": ["one", "two"]},
        }]));
        assert_eq!(cells("analysis.ipynb", &text), BTreeSet::from([("Python", Kind::Code, 1)]));
    }

    #[test]
    fn script_and_style_content_counts_under_javascript_and_css() {
        let text = "<style>\n  p { color: red; }\n</style>\n<script>\n  // greet\n  console.log(\"hi\");\n</script>\n<p>done</p>\n";
        let expected = BTreeSet::from([
            ("HTML", Kind::Code, 5),
            ("CSS", Kind::Code, 1),
            ("JavaScript", Kind::Comment, 1),
            ("JavaScript", Kind::Code, 1),
        ]);
        assert_eq!(cells("page.html", text), expected);
    }

    #[test]
    fn a_script_that_opens_and_closes_on_one_line_stays_html() {
        assert_eq!(cells("page.html", "<script src=\"app.js\"></script>\n"), BTreeSet::from([("HTML", Kind::Code, 1)]));
    }

    #[test]
    fn embedded_lines_keep_the_role_of_their_file() {
        let counted = file("docs/guide.md", b"```python\nvalue = 1\n```\n");
        assert!(counted.cells.contains(&cell("Python", Role::Documentation, Kind::Code, 1)));
    }
}
