//! The terminal view: every section opens with its head — a row per role group, the kind
//! groups and the test share — then the tables of the plain text, coloured, with a share bar per row.
//!
//! The bar measures a row against the largest row of its table, so the largest fills it; the
//! percentage beside it is the row's share of all lines. The view fits the terminal: when the
//! full layout is too wide it gives way step by step — narrower gaps and a shorter bar, the
//! frame, the percentage, then compact numbers with long names cut, and the bar last. The
//! dense view starts at compact numbers.

use crate::render::{self, Head, Row, Section, Table};

const SLIVER: char = '▏';
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[90m";
const FRAME: &str = "\x1b[31m";
const TESTS: &str = "\x1b[32m";

/// The colour of a column, a kind or a kind group.
fn colour(column: &str) -> &'static str {
    match column {
        "code" => "\x1b[36m",
        "docstring" | "docs" => "\x1b[32m",
        "comment" => "\x1b[33m",
        "prose" => "\x1b[35m",
        "data" => "\x1b[34m",
        "blank" => DIM,
        _ => "\x1b[37m",
    }
}

/// The colour of a role group.
fn group_colour(group: &str) -> &'static str {
    match group {
        "engineering" => "\x1b[36m",
        "documentation" => "\x1b[35m",
        "data" => "\x1b[34m",
        _ => DIM,
    }
}

/// How the view is laid out: dense or not, and the width it must fit.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub dense: bool,
    pub width: usize,
}

/// One step of the cascade from the full view to the narrowest.
#[derive(Clone, Copy, Debug)]
struct Style {
    frame: bool,
    compact: bool,
    gap: usize,
    bar: usize,
    percent: bool,
    /// The widest a row name may be before it is cut.
    names: usize,
}

const FULL: Style = Style { frame: true, compact: false, gap: 3, bar: 24, percent: true, names: usize::MAX };
const DENSE_START: Style = Style { frame: false, compact: true, gap: 1, bar: 12, percent: false, names: 14 };
const CASCADE: &[Style] = &[
    FULL,
    Style { gap: 2, bar: 12, ..FULL },
    Style { gap: 1, bar: 8, ..FULL },
    Style { frame: false, gap: 1, bar: 8, ..FULL },
    Style { frame: false, gap: 1, bar: 8, percent: false, ..FULL },
    DENSE_START,
    Style { bar: 8, names: 13, ..DENSE_START },
    Style { bar: 6, names: 12, ..DENSE_START },
    Style { bar: 0, names: 12, ..DENSE_START },
];
/// Where the dense view enters the cascade.
const DENSE: usize = 5;

/// Render every section; the result holds escape sequences and ends with a newline.
pub fn show(sections: &[Section], layout: Layout) -> String {
    let start = if layout.dense { DENSE } else { 0 };
    let style = CASCADE[start..]
        .iter()
        .copied()
        .find(|style| sections.iter().all(|section| width(&render_section(section, *style, None)) <= layout.width))
        .unwrap_or(CASCADE[CASCADE.len() - 1]);
    sections.iter().map(|section| render_section(section, style, Some(layout.width))).collect::<Vec<_>>().join("\n")
}

fn width(text: &str) -> usize {
    text.lines().map(visible_width).max().unwrap_or(0)
}

/// Render one section as `style` draws it; without a `limit` it is only measured, and names stay as the style caps them.
fn render_section(section: &Section, style: Style, limit: Option<usize>) -> String {
    let inner = limit.map(|limit| if style.frame { limit.saturating_sub(4) } else { limit });
    let mut tables: Vec<String> = Vec::new();
    for table in &section.tables {
        tables.extend(render_table(table, style, inner));
        tables.push(String::new());
    }
    let mut lines = head(&section.head, style);
    lines.push(String::new());
    lines.extend(tables);
    // The notes and the title give way to the tables: they are cut to the width, never measured.
    if let Some(inner) = inner {
        let excluded = (!section.excluded.is_empty()).then(|| render::excluded_text(&section.excluded));
        let unparsed = section.unparsed.iter().map(|path| format!("unparsed: {path}"));
        let notes = excluded.into_iter().chain(unparsed).chain(section.since.iter().cloned());
        lines.extend(notes.map(|note| format!("{DIM}{}{RESET}", cut(&note, inner))));
    }
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let number = if style.compact { compact(section.head.lines) } else { thousands(section.head.lines) };
    let count = format!(" · {number} lines");
    let name =
        inner.map_or_else(String::new, |inner| cut(&section.title, inner.saturating_sub(count.chars().count() + 2)));
    let title = name + &count;
    if style.frame {
        frame(&title, &lines)
    } else {
        std::iter::once(format!("{BOLD}{title}{RESET}")).chain(lines).map(|line| line + "\n").collect()
    }
}

/// A rounded frame around the lines, the title set into its top edge.
fn frame(title: &str, lines: &[String]) -> String {
    let title_width = title.chars().count();
    let inner = lines.iter().map(|line| visible_width(line)).max().unwrap_or(0).max(title_width + 2);
    let mut out =
        format!("{FRAME}╭─{RESET} {BOLD}{title}{RESET} {FRAME}{}╮{RESET}\n", "─".repeat(inner - title_width - 1));
    for line in lines {
        let pad = " ".repeat(inner - visible_width(line));
        out.push_str(&format!("{FRAME}│{RESET} {line}{pad} {FRAME}│{RESET}\n"));
    }
    out.push_str(&format!("{FRAME}╰{}╯{RESET}\n", "─".repeat(inner + 2)));
    out
}

/// The number of characters a line shows.
fn visible_width(line: &str) -> usize {
    strip(line).chars().count()
}

/// Text without its escape sequences.
pub fn strip(text: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    for character in text.chars() {
        match (escaped, character) {
            (false, '\x1b') => escaped = true,
            (true, 'm') => escaped = false,
            (true, _) => {}
            (false, _) => out.push(character),
        }
    }
    out
}

/// The head: a row per role group with its share of all lines, then the kind groups and the test share.
fn head(head: &Head, style: Style) -> Vec<String> {
    let number = |value: u64| if style.compact { compact(value) } else { thousands(value) };
    let grand = head.lines.max(1);
    let names = head.roles.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let digits = head.roles.iter().map(|(_, lines)| number(*lines).len()).max().unwrap_or(0);
    let gap = " ".repeat(style.gap);
    let mut lines = Vec::new();
    for (name, count) in &head.roles {
        let mut line = format!("{BOLD}{name:<names$}{RESET}");
        if style.bar > 0 {
            let (painted, drawn) = bar(&[(group_colour(name), *count)], grand, style.bar);
            line.push_str(&format!("{gap}{painted}{}", " ".repeat(style.bar - drawn)));
        }
        let share = *count as f64 / grand as f64 * 100.0;
        line.push_str(&format!("{gap}{share:>5.1}%{gap}{:>digits$}", number(*count)));
        lines.push(line);
    }
    lines.push(String::new());
    let kinds: Vec<String> =
        head.kinds.iter().map(|(name, count)| format!("{}{name}{RESET} {}", colour(name), compact(*count))).collect();
    lines.push(kinds.join(&format!(" {DIM}·{RESET} ")));
    lines.extend(render::tests_text(head.tests, compact).map(|text| format!("{TESTS}{text}{RESET}")));
    lines
}

fn render_table(table: &Table, style: Style, limit: Option<usize>) -> Vec<String> {
    let number = |value: u64| if style.compact { compact(value) } else { thousands(value) };
    let grand = table.total.total().max(1);
    let largest = table.rows.iter().map(Row::total).max().unwrap_or(0).max(1);
    let rows: Vec<&Row> = table.rows.iter().chain([&table.total]).collect();

    let mut grid: Vec<Vec<(String, &'static str)>> = vec![
        std::iter::once((table.heading.to_owned(), BOLD))
            .chain(table.columns.iter().map(|column| ((*column).to_owned(), BOLD)))
            .chain([("total".to_owned(), BOLD)])
            .chain(table.scored.then(|| ("tests".to_owned(), BOLD)))
            .collect(),
    ];
    for (index, row) in rows.iter().enumerate() {
        let is_total = index == table.rows.len();
        let mut cells = vec![(row.name.clone(), if is_total { BOLD } else { "" })];
        for (column, value) in table.columns.iter().zip(&row.counts) {
            cells.push(if *value == 0 { ("0".to_owned(), DIM) } else { (number(*value), colour(column)) });
        }
        cells.push((number(row.total()), BOLD));
        if table.scored {
            cells.push((render::share_text(row.tests), TESTS));
        }
        grid.push(cells);
    }
    let columns = grid[0].len();
    let mut widths: Vec<usize> =
        (0..columns).map(|column| grid.iter().map(|row| row[column].0.chars().count()).max().unwrap_or(0)).collect();
    let whole = widths[0];
    widths[0] = whole.min(style.names);
    let share_width = if style.bar > 0 { style.gap + style.bar + if style.percent { 7 } else { 0 } } else { 0 };
    let used = widths.iter().sum::<usize>() + style.gap * (columns - 1) + share_width;
    // The names give way last: they take what the width leaves, or lose what it still lacks.
    if let Some(limit) = limit {
        widths[0] = if used > limit {
            widths[0].saturating_sub(used - limit).max(4)
        } else {
            whole.min(widths[0] + limit - used)
        };
    }
    let table_width = widths.iter().sum::<usize>() + style.gap * (columns - 1) + share_width;

    let mut lines = Vec::new();
    for (index, cells) in grid.iter().enumerate() {
        if index == rows.len() {
            lines.push(String::new());
        }
        let mut text = String::new();
        for (column, ((value, paint), width)) in cells.iter().zip(&widths).enumerate() {
            let shown = cut(value, *width);
            let pad = " ".repeat(width - shown.chars().count());
            let painted = if paint.is_empty() { shown } else { format!("{paint}{shown}{RESET}") };
            if column == 0 {
                text.push_str(&painted);
                text.push_str(&pad);
            } else {
                text.push_str(&" ".repeat(style.gap));
                text.push_str(&pad);
                text.push_str(&painted);
            }
        }
        if style.bar > 0 && index == 0 {
            text.push_str(&format!("{}{BOLD}share{RESET}", " ".repeat(style.gap)));
        } else if style.bar > 0 && (1..rows.len()).contains(&index) {
            let row = rows[index - 1];
            let parts: Vec<(&'static str, u64)> =
                table.columns.iter().map(|column| colour(column)).zip(row.counts.iter().copied()).collect();
            let (painted, drawn) = bar(&parts, largest, style.bar);
            text.push_str(&" ".repeat(style.gap));
            text.push_str(&painted);
            if style.percent {
                let share = row.total() as f64 / grand as f64 * 100.0;
                text.push_str(&format!("{}{DIM}{share:>6.1}%{RESET}", " ".repeat(style.bar - drawn)));
            }
        }
        lines.push(text);
        if index == 0 {
            lines.push(format!("{DIM}{}{RESET}", "─".repeat(table_width)));
        }
    }
    lines
}

/// The cells of the progress bar.
const PROGRESS: usize = 30;

/// The progress of a fleet run on one line: a bar of the finished share, the count, and the
/// directories still being scanned — the slow ones are the ones it names last.
pub fn progress(done: usize, total: usize, running: &[String], width: usize) -> String {
    let filled = (done * PROGRESS).checked_div(total).unwrap_or(PROGRESS);
    let count = format!("{done:>digits$}/{total}", digits = total.to_string().len());
    let line = format!(
        "freightscan {FRAME}{}{RESET}{DIM}{}{RESET} {count}",
        "━".repeat(filled),
        "─".repeat(PROGRESS - filled)
    );
    let room = width.saturating_sub(visible_width(&line) + 2);
    match running.join(", ") {
        names if names.is_empty() || room < 4 => line,
        names => format!("{line}  {DIM}{}{RESET}", cut(&names, room)),
    }
}

/// Text no wider than `width`, its end replaced by an ellipsis when it is cut.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars().take(width.saturating_sub(1)).chain(['…']).collect()
}

/// A bar as long as the row against the largest row, one stretch per coloured part, and the cells it takes.
///
/// Rounding the running sum instead of each part keeps the bar's length true to the row;
/// a row too small for one full block still shows a sliver.
fn bar(parts: &[(&'static str, u64)], largest: u64, width: usize) -> (String, usize) {
    let mut painted = String::new();
    let mut running = 0u64;
    let mut drawn = 0usize;
    for (paint, lines) in parts {
        running += lines;
        let end = (running as f64 / largest as f64 * width as f64).round() as usize;
        if end > drawn {
            painted.push_str(&format!("{paint}{}{RESET}", "█".repeat(end - drawn)));
            drawn = end;
        }
    }
    if drawn == 0 && running > 0 {
        let biggest = parts.iter().max_by_key(|(_, lines)| *lines).map_or(DIM, |(paint, _)| *paint);
        painted.push_str(&format!("{biggest}{SLIVER}{RESET}"));
        drawn = 1;
    }
    (painted, drawn)
}

/// A count with thousands separators: `29,611`.
fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A count in at most five characters: `950`, `29.6k`, `133k`, `1.3M`, `123M`.
fn compact(value: u64) -> String {
    match value {
        0..1_000 => value.to_string(),
        1_000..99_950 => format!("{:.1}k", value as f64 / 1_000.0),
        99_950..999_500 => format!("{:.0}k", value as f64 / 1_000.0),
        999_500..99_950_000 => format!("{:.1}M", value as f64 / 1_000_000.0),
        _ => format!("{:.0}M", value as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "\x1b[36m";

    #[test]
    fn shall_41_progress_fills_its_share_and_names_what_still_runs() {
        let running = ["atlas".to_owned(), "beacon".to_owned()];
        let line = progress(29, 58, &running, 100);
        assert_eq!(line.matches('━').count(), PROGRESS / 2);
        assert!(line.contains("29/58") && line.contains("atlas, beacon"), "{line}");
        assert!(visible_width(&progress(29, 58, &running, 50)) <= 50);
        assert_eq!(progress(58, 58, &[], 100).matches('━').count(), PROGRESS);
    }

    #[test]
    fn shall_40_the_largest_row_fills_the_bar() {
        assert_eq!(bar(&[(CODE, 30), (DIM, 10)], 40, 24).1, 24);
    }

    #[test]
    fn shall_40_a_row_too_small_for_a_block_still_shows_a_sliver() {
        let (painted, drawn) = bar(&[(CODE, 1)], 10_000, 24);
        assert_eq!(drawn, 1);
        assert!(painted.contains(SLIVER));
    }

    #[test]
    fn an_empty_row_draws_nothing() {
        assert_eq!(bar(&[], 10, 24).1, 0);
    }

    #[test]
    fn numbers_read_with_separators_or_compact() {
        assert_eq!(thousands(132_794), "132,794");
        assert_eq!(thousands(950), "950");
        assert_eq!(compact(950), "950");
        assert_eq!(compact(29_611), "29.6k");
        assert_eq!(compact(99_999), "100k");
        assert_eq!(compact(132_794), "133k");
        assert_eq!(compact(999_999), "1.0M");
        assert_eq!(compact(123_456_789), "123M");
        assert_eq!(compact(1_250_000), "1.2M");
    }

    #[test]
    fn the_visible_width_leaves_escape_sequences_out() {
        assert_eq!(visible_width(&format!("{BOLD}total{RESET}")), 5);
    }

    #[test]
    fn shall_47_the_head_ranks_the_role_groups_and_names_kinds_and_tests() {
        let scan = crate::count::assemble(
            std::path::PathBuf::from("repository"),
            [
                crate::count::count_file("src/probe.py", b"a = 1\nb = 2\nc = 3\n"),
                crate::count::count_file("tests/test_probe.py", b"assert a\n"),
                crate::count::count_file("README.md", b"Words.\n"),
            ],
        );
        let sections = render::sections(&[scan], render::Options::default(), &render::Previous::new());
        let lines: Vec<String> = head(&sections[0].head, FULL).iter().map(|line| strip(line)).collect();
        let engineering = lines.iter().find(|line| line.starts_with("engineering")).unwrap();
        // 80 % of 24 cells.
        assert!(
            engineering.contains(&format!("{} ", "█".repeat(19))) && engineering.contains("80.0%"),
            "{engineering}"
        );
        assert!(lines.iter().any(|line| line.starts_with("documentation") && line.contains("20.0%")));
        assert!(lines.contains(&"code 4 · docs 1".to_owned()), "{lines:?}");
        assert!(lines.contains(&"tests 25.0% — 1 test code beside 3 source code".to_owned()), "{lines:?}");
    }

    #[test]
    fn a_name_keeps_all_the_room_the_width_leaves_it() {
        let path = "application/apps/accounts/migrations/0001_initial_accounts.py";
        let scan = crate::count::assemble(
            std::path::PathBuf::from("repository"),
            [crate::count::count_file(path, b"a = 1\nb = 2\n")],
        );
        let options = render::Options { files: true, ..render::Options::default() };
        let sections = render::sections(&[scan], options, &render::Previous::new());
        let shown = strip(&show(&sections, Layout { dense: false, width: 100 }));
        assert!(shown.contains(path), "{shown}");
    }

    #[test]
    fn a_long_root_costs_the_view_nothing() {
        let scan = crate::count::assemble(
            std::path::PathBuf::from("deep/".repeat(40)),
            [crate::count::count_file("probe.py", b"a = 1\n")],
        );
        let sections = render::sections(&[scan], render::Options::default(), &render::Previous::new());
        let shown = strip(&show(&sections, Layout { dense: false, width: 79 }));
        assert!(shown.contains('╭') && shown.contains("1 lines"), "{shown}");
        assert!(shown.lines().all(|line| line.chars().count() <= 79), "{shown}");
    }

    #[test]
    fn a_cut_name_ends_in_an_ellipsis() {
        assert_eq!(cut("JSON with comments", 8), "JSON wi…");
        assert_eq!(cut("Python", 8), "Python");
    }
}
