//! Sums over scans — every table and every JSON field is derived here, from the matrix.

use std::collections::BTreeMap;

use crate::languages;
use crate::types::{Cell, Exclusion, FileCount, Kind, Role, Scan};

/// Lines per kind; a kind without lines has no entry.
pub type Counts = BTreeMap<Kind, u64>;

/// Rows of a table in the order they are shown.
pub type Rows = Vec<(String, Counts)>;

/// Sum the lines of every file into one cell per language, role and kind.
pub fn matrix<'scan>(scans: impl IntoIterator<Item = &'scan Scan>) -> Vec<Cell> {
    let mut tally: BTreeMap<(&'static str, Role, Kind), u64> = BTreeMap::new();
    for scan in scans {
        for file in &scan.files {
            for cell in &file.cells {
                *tally.entry((cell.language, cell.role, cell.kind)).or_default() += cell.lines;
            }
        }
    }
    tally.into_iter().map(|((language, role, kind), lines)| Cell { language, role, kind, lines }).collect()
}

/// Lines per language and kind, the language with the most code first.
pub fn by_language(cells: &[Cell]) -> Rows {
    let mut rows: Rows = group(cells, |cell| cell.language.to_owned()).into_iter().collect();
    rows.sort_by(|(left, left_counts), (right, right_counts)| {
        let code = |counts: &Counts| counts.get(&Kind::Code).copied().unwrap_or_default();
        code(right_counts).cmp(&code(left_counts)).then_with(|| left.cmp(right))
    });
    rows
}

/// Lines per role and kind, in the order of the role table.
pub fn by_role(cells: &[Cell]) -> Vec<(Role, Counts)> {
    let mut table = group(cells, |cell| cell.role);
    Role::ALL.into_iter().filter_map(|role| table.remove(&role).map(|counts| (role, counts))).collect()
}

/// Lines per language and role, the matrix as table rows.
pub fn by_language_and_role(cells: &[Cell]) -> Rows {
    group(cells, |cell| (cell.language, cell.role))
        .into_iter()
        .map(|((language, role), counts)| (format!("{language} · {role}"), counts))
        .collect()
}

/// What the file table is ordered by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    /// The path, alphabetically.
    #[default]
    Path,
    /// All lines of the file.
    Lines,
    /// The file's code lines.
    Code,
    /// The role, in the order of the role table; the larger file first within one role.
    Role,
    /// The file's own language; the larger file first within one language.
    Language,
}

/// The order of the file table: a key and its direction; `None` takes the key's natural one.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sort {
    pub key: SortKey,
    pub descending: Option<bool>,
}

/// Lines per file and kind, in the order `sort` asks for.
pub fn by_file(scan: &Scan, sort: Sort) -> Rows {
    sorted_files(scan, sort).into_iter().map(|file| (file.path.clone(), total(&file.cells))).collect()
}

/// The counted files in the order `sort` asks for.
///
/// Counts sort largest first and names alphabetically unless the direction says otherwise.
pub fn sorted_files(scan: &Scan, sort: Sort) -> Vec<&FileCount> {
    let mut files: Vec<_> = scan.files.iter().filter(|file| !file.cells.is_empty()).collect();
    let lines = |file: &&FileCount| file.cells.iter().map(|cell| cell.lines).sum::<u64>();
    let code = |file: &&FileCount| {
        file.cells.iter().filter(|cell| cell.kind == Kind::Code).map(|cell| cell.lines).sum::<u64>()
    };
    files.sort_by(|left, right| match sort.key {
        SortKey::Path => left.path.cmp(&right.path),
        SortKey::Lines => lines(right).cmp(&lines(left)),
        SortKey::Code => code(right).cmp(&code(left)),
        SortKey::Role => left.cells[0].role.cmp(&right.cells[0].role).then_with(|| lines(right).cmp(&lines(left))),
        SortKey::Language => {
            left.cells[0].language.cmp(right.cells[0].language).then_with(|| lines(right).cmp(&lines(left)))
        }
    });
    let natural = matches!(sort.key, SortKey::Lines | SortKey::Code);
    if sort.descending.is_some_and(|descending| descending != natural) {
        files.reverse();
    }
    files
}

/// Sum cells again under the family of their language: a dialect counts with the language it extends.
pub fn fold(cells: Vec<Cell>) -> Vec<Cell> {
    let mut tally: BTreeMap<(&'static str, Role, Kind), u64> = BTreeMap::new();
    for cell in cells {
        *tally.entry((languages::family(cell.language), cell.role, cell.kind)).or_default() += cell.lines;
    }
    tally.into_iter().map(|((language, role, kind), lines)| Cell { language, role, kind, lines }).collect()
}

/// Lines per kind over all cells.
pub fn total(cells: &[Cell]) -> Counts {
    let mut counts = Counts::new();
    for cell in cells {
        *counts.entry(cell.kind).or_default() += cell.lines;
    }
    counts
}

/// Code lines of tests and of source: how much of the code is test code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tests {
    pub test: u64,
    pub source: u64,
}

impl Tests {
    /// The tests' share of test and source code together, in percent; None when there is neither.
    pub fn share(self) -> Option<f64> {
        let both = self.test + self.source;
        (both > 0).then(|| self.test as f64 / both as f64 * 100.0)
    }
}

/// The code lines of tests and of source over all cells.
pub fn tests(cells: &[Cell]) -> Tests {
    let code = |role: Role| -> u64 {
        cells.iter().filter(|cell| cell.kind == Kind::Code && cell.role == role).map(|cell| cell.lines).sum()
    };
    Tests { test: code(Role::Test), source: code(Role::Source) }
}

/// Excluded files per reason over all scans.
pub fn excluded<'scan>(scans: impl IntoIterator<Item = &'scan Scan>) -> BTreeMap<Exclusion, u64> {
    let mut tally = BTreeMap::new();
    for scan in scans {
        for (reason, count) in &scan.excluded {
            *tally.entry(*reason).or_default() += count;
        }
    }
    tally
}

/// Python files that fell back to comment markers, in path order.
pub fn unparsed(scan: &Scan) -> Vec<String> {
    let mut paths: Vec<String> = scan.files.iter().filter(|file| file.unparsed).map(|file| file.path.clone()).collect();
    paths.sort();
    paths
}

/// Group cells by `key` and sum their lines per kind.
fn group<Key: Ord>(cells: &[Cell], key: impl Fn(&Cell) -> Key) -> BTreeMap<Key, Counts> {
    let mut table: BTreeMap<Key, Counts> = BTreeMap::new();
    for cell in cells {
        *table.entry(key(cell)).or_default().entry(cell.kind).or_default() += cell.lines;
    }
    table
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::count;

    fn scan(files: &[(&str, &[u8])]) -> Scan {
        count::assemble(PathBuf::from("repository"), files.iter().map(|(path, data)| count::count_file(path, data)))
    }

    #[test]
    fn the_test_share_weighs_test_code_against_source_code() {
        let repository = scan(&[
            ("src/probe.py", b"\"\"\"Doc.\"\"\"\na = 1\nb = 2\nc = 3\n"),
            ("tests/test_probe.py", b"assert True\n"),
            ("pyproject.toml", b"[project]\n"),
        ]);
        let tests = tests(&matrix([&repository]));
        assert_eq!(tests, Tests { test: 1, source: 3 });
        assert_eq!(tests.share(), Some(25.0));
        assert_eq!(Tests::default().share(), None);
    }

    fn order(rows: Rows) -> Vec<String> {
        rows.into_iter().map(|(path, _)| path).collect()
    }

    #[test]
    fn files_sort_by_key_and_direction() {
        let repository = scan(&[
            ("b.py", b"a = 1\nb = 2\nc = 3\n"),
            ("a.py", b"a = 1\n"),
            ("tests/test_c.py", b"a = 1\nb = 2\n"),
            ("README.md", b"Words.\nMore.\nStill.\nEnd.\n"),
        ]);
        let by = |key, descending| order(by_file(&repository, Sort { key, descending }));
        assert_eq!(by(SortKey::Path, None), ["README.md", "a.py", "b.py", "tests/test_c.py"]);
        assert_eq!(by(SortKey::Path, Some(true)), ["tests/test_c.py", "b.py", "a.py", "README.md"]);
        assert_eq!(by(SortKey::Lines, None), ["README.md", "b.py", "tests/test_c.py", "a.py"]);
        assert_eq!(by(SortKey::Lines, Some(false)), ["a.py", "tests/test_c.py", "b.py", "README.md"]);
        assert_eq!(by(SortKey::Code, None)[0], "b.py");
        assert_eq!(by(SortKey::Role, None), ["b.py", "a.py", "tests/test_c.py", "README.md"]);
        assert_eq!(by(SortKey::Language, None), ["README.md", "b.py", "tests/test_c.py", "a.py"]);
    }

    #[test]
    fn dialects_fold_into_their_family() {
        let repository =
            scan(&[("a.json", b"{}\n"), ("b.jsonc", b"// why\n{}\n"), ("Caddyfile", b"a {\n}\n"), (".env", b"A=1\n")]);
        let folded: Vec<_> = by_language(&fold(matrix([&repository]))).into_iter().map(|(name, _)| name).collect();
        assert_eq!(folded, ["Configuration", "JSON"]);
        let detailed: Vec<_> = by_language(&matrix([&repository])).into_iter().map(|(name, _)| name).collect();
        assert_eq!(detailed, ["Caddyfile", "JSON", "JSON with comments", "env"]);
    }
}
