//! Turn scans into tables, plain text and the JSON document — no colours, no I/O.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::report::{self, Counts, Rows, Tests};
use crate::types::{Cell, Exclusion, Kind, Role, Scan};
use crate::{groups, snapshot};

const SCHEMA: u64 = 3;
/// How many languages the folded view names before it sums the rest as `others`.
const NAMED_LANGUAGES: usize = 8;

/// The latest earlier snapshot record per scanned root, when a snapshot file was given.
pub type Previous = BTreeMap<String, Value>;

/// What to show beside the head and the language table.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub matrix: bool,
    pub files: bool,
    /// A section per directory before the combined one; without it only the combined section shows.
    pub each: bool,
    /// Every kind, role and language apart; without it they fold into groups, families and the
    /// largest languages.
    pub detailed: bool,
    /// The test share of every directory beside its counts.
    pub tests: bool,
    pub sort: report::Sort,
}

impl Options {
    /// The matrix of the scans, folded into families unless the detailed view is asked for.
    fn cells<'scan>(&self, scans: impl IntoIterator<Item = &'scan Scan>) -> Vec<Cell> {
        let cells = report::matrix(scans);
        if self.detailed { cells } else { report::fold(cells) }
    }
}

/// One row of a table: its lines per column, and its test share where the table carries one.
#[derive(Debug)]
pub struct Row {
    pub name: String,
    pub counts: Vec<u64>,
    pub tests: Option<Tests>,
}

impl Row {
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }
}

/// One table: what its rows are, the columns that occur, the rows and their total.
#[derive(Debug)]
pub struct Table {
    pub heading: &'static str,
    /// Kinds in the detailed view, kind groups otherwise.
    pub columns: Vec<&'static str>,
    pub rows: Vec<Row>,
    pub total: Row,
    /// Whether the rows carry their test share.
    pub scored: bool,
}

/// The top of a section: its lines per role group and per kind group, and the test share.
#[derive(Debug)]
pub struct Head {
    pub lines: u64,
    pub roles: Vec<(&'static str, u64)>,
    pub kinds: Vec<(&'static str, u64)>,
    pub tests: Tests,
}

/// Everything shown for one directory, or for several combined.
#[derive(Debug)]
pub struct Section {
    pub title: String,
    pub head: Head,
    pub tables: Vec<Table>,
    pub excluded: BTreeMap<Exclusion, u64>,
    pub unparsed: Vec<String>,
    pub since: Vec<String>,
}

/// One section per scan, and a combined one when there are several.
pub fn sections(scans: &[Scan], options: Options, previous: &Previous) -> Vec<Section> {
    let mut result: Vec<Section> = Vec::new();
    if options.each || scans.len() == 1 {
        for scan in scans {
            let change = since(scan, previous).map(|(at, changes)| since_text(&at, &changes));
            result.push(section(scan.root_text(), std::slice::from_ref(scan), options, change.into_iter().collect()));
        }
    }
    if scans.len() > 1 {
        let combined = Options { files: false, ..options };
        // Without a section of its own, a directory reports its change here.
        let changes = scans.iter().filter(|_| !options.each).filter_map(|scan| {
            since(scan, previous).map(|(at, changes)| format!("{}: {}", scan.name, since_text(&at, &changes)))
        });
        let mut all = section(format!("all {} directories", scans.len()), scans, combined, changes.collect());
        all.tables.insert(0, directories(scans, options));
        result.push(all);
    }
    result
}

/// Render every section as right-aligned text tables.
pub fn plain(scans: &[Scan], options: Options, previous: &Previous) -> String {
    let blocks: Vec<String> = sections(scans, options, previous)
        .into_iter()
        .map(|section| {
            let mut lines = vec![format!("{} · {} lines", section.title, section.head.lines), String::new()];
            lines.extend(plain_head(&section.head));
            for table in &section.tables {
                lines.push(String::new());
                lines.extend(plain_table(table));
            }
            if !section.excluded.is_empty() {
                lines.push(String::new());
                lines.push(excluded_text(&section.excluded));
            }
            lines.extend(section.unparsed.iter().map(|path| format!("unparsed: {path}")));
            lines.extend(section.since);
            lines.join("\n")
        })
        .collect();
    blocks.join("\n\n") + "\n"
}

/// Build the JSON document: the tables of every scan, and the combined ones for several.
pub fn document(scans: &[Scan], options: Options, previous: &Previous) -> Value {
    let mut entries = Vec::new();
    for scan in scans {
        let cells = options.cells([scan]);
        let mut entry = Map::new();
        entry.insert("root".into(), json!(scan.root_text()));
        entry.extend(json_tables(&cells, std::slice::from_ref(scan)));
        if options.tests {
            entry.insert("tests".into(), json_tests(report::tests(&cells)));
        }
        entry.insert("unparsed".into(), json!(report::unparsed(scan)));
        if let Some((at, changes)) = since(scan, previous) {
            let roles: Map<String, Value> =
                changes.iter().map(|(role, lines)| (role.name().to_owned(), json!(lines))).collect();
            entry.insert("since".into(), json!({"at": at, "roles": roles}));
        }
        if options.matrix {
            let matrix: Vec<Value> = cells
                .iter()
                .map(|cell| json!([cell.language, cell.role.name(), cell.kind.name(), cell.lines]))
                .collect();
            entry.insert("matrix".into(), json!(matrix));
        }
        if options.files {
            let files: Vec<Value> = report::sorted_files(scan, options.sort)
                .into_iter()
                .map(|file| {
                    let cells: Vec<Value> = file
                        .cells
                        .iter()
                        .map(|cell| json!([cell.language, cell.role.name(), cell.kind.name(), cell.lines]))
                        .collect();
                    json!({"path": file.path, "role": file.cells[0].role.name(), "cells": cells})
                })
                .collect();
            entry.insert("files".into(), json!(files));
        }
        entries.push(Value::Object(entry));
    }
    let mut result = Map::new();
    result.insert("schema".into(), json!(SCHEMA));
    result.insert("version".into(), json!(env!("CARGO_PKG_VERSION")));
    result.insert("scans".into(), Value::Array(entries));
    if scans.len() > 1 {
        let cells = options.cells(scans);
        let mut combined = json_tables(&cells, scans);
        if options.tests {
            combined.insert("tests".into(), json_tests(report::tests(&cells)));
        }
        result.insert("combined".into(), Value::Object(combined));
    }
    Value::Object(result)
}

/// The excluded files as one line: `excluded: binary 2, lock 1`.
pub fn excluded_text(excluded: &BTreeMap<Exclusion, u64>) -> String {
    let parts: Vec<String> = excluded.iter().map(|(reason, count)| format!("{reason} {count}")).collect();
    format!("excluded: {}", parts.join(", "))
}

/// The test share as one line: `tests 43.7% — 194k test code beside 250k source code`.
pub fn tests_text(tests: Tests, number: impl Fn(u64) -> String) -> Option<String> {
    let share = tests.share()?;
    Some(format!("tests {share:.1}% — {} test code beside {} source code", number(tests.test), number(tests.source)))
}

/// The columns of a section's tables and how a kind finds its column.
struct Shape {
    columns: Vec<&'static str>,
    detailed: bool,
}

impl Shape {
    /// The columns that hold lines: every kind apart when detailed, else the kind groups.
    fn new(totals: &Counts, detailed: bool) -> Shape {
        let order: Vec<&'static str> =
            if detailed { Kind::ALL.iter().map(|kind| kind.name()).collect() } else { groups::KINDS.to_vec() };
        let used = |column: &&str| totals.keys().any(|kind| Shape::column_of(*kind, detailed) == *column);
        Shape { columns: order.into_iter().filter(used).collect(), detailed }
    }

    fn column_of(kind: Kind, detailed: bool) -> &'static str {
        if detailed { kind.name() } else { groups::kind(kind) }
    }

    fn row(&self, name: String, counts: &Counts) -> Row {
        let sum = |column: &&str| -> u64 {
            counts
                .iter()
                .filter(|(kind, _)| Shape::column_of(**kind, self.detailed) == *column)
                .map(|(_, lines)| lines)
                .sum()
        };
        Row { name, counts: self.columns.iter().map(sum).collect(), tests: None }
    }

    fn table(&self, heading: &'static str, rows: Rows, totals: &Counts) -> Table {
        Table {
            heading,
            columns: self.columns.clone(),
            rows: rows.into_iter().map(|(name, counts)| self.row(name, &counts)).collect(),
            total: self.row("total".to_owned(), totals),
            scored: false,
        }
    }
}

fn section(title: String, scans: &[Scan], options: Options, since: Vec<String>) -> Section {
    let cells = options.cells(scans);
    let totals = report::total(&cells);
    let shape = Shape::new(&totals, options.detailed);
    let mut tables = Vec::new();
    if options.detailed {
        let roles = report::by_role(&cells).into_iter().map(|(role, counts)| (role.name().to_owned(), counts));
        tables.push(shape.table("role", roles.collect(), &totals));
    }
    let languages = report::by_language(&cells);
    let languages = if options.detailed { languages } else { largest(languages, NAMED_LANGUAGES) };
    tables.push(shape.table("language", languages, &totals));
    if options.matrix {
        tables.push(shape.table("language · role", report::by_language_and_role(&cells), &totals));
    }
    if options.files {
        for scan in scans {
            let rows = report::by_file(scan, options.sort).into_iter().map(|(path, counts)| (printable(&path), counts));
            tables.push(shape.table("file", rows.collect(), &totals));
        }
    }
    let unparsed = scans.iter().flat_map(report::unparsed).map(|path| printable(&path)).collect();
    Section { title, head: head(&cells), tables, excluded: report::excluded(scans), unparsed, since }
}

/// A path fit for the terminal: a file name may hold control characters, a hostile one escape sequences.
fn printable(path: &str) -> String {
    path.chars().map(|character| if character.is_control() { '\u{fffd}' } else { character }).collect()
}

/// The `named` rows with the most lines, the rest summed into one row `others`.
fn largest(mut rows: Rows, named: usize) -> Rows {
    let lines = |counts: &Counts| counts.values().sum::<u64>();
    rows.sort_by(|left, right| lines(&right.1).cmp(&lines(&left.1)).then_with(|| left.0.cmp(&right.0)));
    if rows.len() <= named + 1 {
        return rows;
    }
    let mut others = Counts::new();
    for (_, counts) in rows.drain(named..) {
        for (kind, lines) in counts {
            *others.entry(kind).or_default() += lines;
        }
    }
    rows.push(("others".to_owned(), others));
    rows
}

/// Lines per role group and kind group, in the groups' order, and the test share.
fn head(cells: &[Cell]) -> Head {
    let groups = |names: &[&'static str], of: &dyn Fn(&Cell) -> &'static str| -> Vec<(&'static str, u64)> {
        names
            .iter()
            .map(|name| (*name, cells.iter().filter(|cell| of(cell) == *name).map(|cell| cell.lines).sum()))
            .filter(|(_, lines)| *lines > 0)
            .collect()
    };
    Head {
        lines: cells.iter().map(|cell| cell.lines).sum(),
        roles: groups(&groups::ROLES, &|cell| groups::role(cell.role)),
        kinds: groups(&groups::KINDS, &|cell| groups::kind(cell.kind)),
        tests: report::tests(cells),
    }
}

/// One row per directory, the largest first, with its test share when asked for.
fn directories(scans: &[Scan], options: Options) -> Table {
    let scored = |cells: &[Cell], name: String, shape: &Shape| Row {
        tests: options.tests.then(|| report::tests(cells)),
        ..shape.row(name, &report::total(cells))
    };
    let cells = report::matrix(scans);
    let shape = Shape::new(&report::total(&cells), options.detailed);
    let mut rows: Vec<Row> =
        scans.iter().map(|scan| scored(&report::matrix([scan]), scan.name.clone(), &shape)).collect();
    rows.sort_by(|left, right| right.total().cmp(&left.total()).then_with(|| left.name.cmp(&right.name)));
    let total = scored(&cells, "total".to_owned(), &shape);
    Table { heading: "directory", columns: shape.columns, rows, total, scored: options.tests }
}

/// The head as plain lines: lines and share per role group, the kind groups, the test share.
fn plain_head(head: &Head) -> Vec<String> {
    let grand = head.lines.max(1) as f64;
    let width = head.roles.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let mut lines: Vec<String> = head
        .roles
        .iter()
        .map(|(name, count)| format!("{name:<width$}  {count:>10}  {:>5.1}%", *count as f64 / grand * 100.0))
        .collect();
    let kinds: Vec<String> = head.kinds.iter().map(|(name, count)| format!("{name} {count}")).collect();
    lines.push(kinds.join(" · "));
    lines.extend(tests_text(head.tests, |count| count.to_string()));
    lines
}

fn json_tests(tests: Tests) -> Value {
    json!({"test": tests.test, "source": tests.source, "share": tests.share()})
}

fn json_tables(cells: &[Cell], scans: &[Scan]) -> Map<String, Value> {
    let counts = |counts: &Counts| -> Value {
        Value::Object(counts.iter().map(|(kind, lines)| (kind.name().to_owned(), json!(lines))).collect())
    };
    let languages: Map<String, Value> =
        report::by_language(cells).iter().map(|(name, row)| (name.clone(), counts(row))).collect();
    let roles: Map<String, Value> =
        report::by_role(cells).iter().map(|(role, row)| (role.name().to_owned(), counts(row))).collect();
    let excluded: Map<String, Value> =
        report::excluded(scans).into_iter().map(|(reason, count)| (reason.name().to_owned(), json!(count))).collect();
    let mut tables = Map::new();
    tables.insert("languages".into(), Value::Object(languages));
    tables.insert("roles".into(), Value::Object(roles));
    tables.insert("excluded".into(), Value::Object(excluded));
    tables.insert("total".into(), counts(&report::total(cells)));
    tables
}

/// The test share of a row as a cell: `58.7%`, or `-` without test or source code.
pub fn share_text(tests: Option<Tests>) -> String {
    tests.and_then(Tests::share).map_or_else(|| "-".to_owned(), |share| format!("{share:.1}%"))
}

/// Right-aligned columns per kind or group and a total column, closed by a total row.
fn plain_table(table: &Table) -> Vec<String> {
    let mut grid: Vec<Vec<String>> = vec![
        std::iter::once(table.heading.to_owned())
            .chain(table.columns.iter().map(|column| (*column).to_owned()))
            .chain(["total".to_owned()])
            .chain(table.scored.then(|| "tests".to_owned()))
            .collect(),
    ];
    for row in table.rows.iter().chain([&table.total]) {
        let numbers = row.counts.iter().map(u64::to_string);
        let share = table.scored.then(|| share_text(row.tests));
        grid.push(
            std::iter::once(row.name.clone()).chain(numbers).chain([row.total().to_string()]).chain(share).collect(),
        );
    }
    let widths: Vec<usize> = (0..grid[0].len())
        .map(|column| grid.iter().map(|row| row[column].chars().count()).max().unwrap_or(0))
        .collect();
    grid.iter()
        .map(|row| {
            let mut line = format!("{:<width$}", row[0], width = widths[0]);
            for (value, width) in row[1..].iter().zip(&widths[1..]) {
                line.push_str(&format!("  {value:>width$}"));
            }
            line
        })
        .collect()
}

/// When and by how many lines per role the scan changed since its latest earlier record.
fn since(scan: &Scan, previous: &Previous) -> Option<(String, Vec<(Role, i64)>)> {
    let record = previous.get(&scan.root_text())?;
    let at = record.get("at")?.as_str()?.to_owned();
    Some((at, snapshot::delta(scan, record)))
}

fn since_text(at: &str, changes: &[(Role, i64)]) -> String {
    let parts: Vec<String> = changes.iter().map(|(role, lines)| format!("{role} {lines:+}")).collect();
    format!("since {at}: {}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::count;

    #[test]
    fn control_characters_of_a_file_name_never_reach_the_screen() {
        let scan =
            count::assemble(PathBuf::from("repository"), [count::count_file("trap\x1b[2J.py", b"def broken(:\n")]);
        let options = Options { files: true, ..Options::default() };
        let shown = plain(&[scan], options, &Previous::new());
        assert!(!shown.contains('\x1b'), "{shown:?}");
        assert_eq!(shown.matches("trap\u{fffd}[2J.py").count(), 2, "{shown}");
    }
}
