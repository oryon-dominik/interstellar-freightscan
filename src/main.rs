//! Counts every line of a repository by language, role and kind.
//!
//! The core is pure functions over text and paths; `adapters` holds git, the file system and
//! the configuration and snapshot files. This file is the command line.

mod adapters;
mod clock;
mod count;
mod display;
mod exclusions;
mod fleet;
mod groups;
mod languages;
mod python;
mod render;
mod report;
mod roles;
mod rust;
mod scanner;
mod sections;
mod snapshot;
mod types;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use rayon::prelude::*;

use crate::render::{Options, Previous};
use crate::types::{Scan, plain_path};

/// The help's colours: headings and switches stand out, placeholders recede.
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Green.on_default())
    .valid(AnsiColor::Cyan.on_default());

#[derive(Parser)]
#[command(name = "freightscan", bin_name = "freightscan", version, styles = STYLES)]
#[command(about = "Count every line of a repository by language, role and kind.")]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Verb>,
    /// Directories to scan; the current one by default.
    paths: Vec<PathBuf>,
    #[command(flatten)]
    output: Output,
}

#[derive(Subcommand)]
enum Verb {
    /// Scan every repository of the list in FREIGHTSCAN_REPOS, one row per repository.
    Fleet {
        /// The repository list; overrides FREIGHTSCAN_REPOS.
        #[arg(long, value_name = "FILE")]
        repos: Option<PathBuf>,
        /// Add a section per repository before the fleet's.
        #[arg(long)]
        each: bool,
        /// Include the forks, marked "owned": false.
        #[arg(long)]
        forks: bool,
        /// Include the private repositories, marked "professional": false.
        #[arg(long)]
        private: bool,
        /// Include forks and private repositories alike.
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: Output,
    },
}

#[derive(Args)]
struct Output {
    /// Output format; rich on a terminal, plain otherwise.
    #[arg(long = "format", value_enum)]
    format: Option<Format>,
    /// Add every count per language, role and kind.
    #[arg(long)]
    matrix: bool,
    /// Add the counts of every file.
    #[arg(long)]
    files: bool,
    /// Fit the terminal view into the terminal's width: no frame, compact numbers.
    #[arg(long)]
    dense: bool,
    /// Show every kind, role and language apart instead of folding them into groups.
    #[arg(long)]
    detailed: bool,
    /// Add the test share of every directory: test code against test and source code.
    #[arg(long)]
    tests: bool,
    /// Order of the file listing; counts run largest first, names alphabetically.
    #[arg(long, value_enum, default_value = "path")]
    sort: SortBy,
    /// Direction of the file listing, overriding the sort key's own.
    #[arg(long, value_enum)]
    order: Option<Order>,
    /// Append a record per directory to this file and report the change since.
    #[arg(long = "snapshot", value_name = "FILE")]
    journal: Option<PathBuf>,
}

impl Output {
    fn options(&self, each: bool) -> Options {
        let sort =
            report::Sort { key: self.sort.into(), descending: self.order.map(|order| matches!(order, Order::Desc)) };
        Options { matrix: self.matrix, files: self.files, each, detailed: self.detailed, tests: self.tests, sort }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Rich,
    Plain,
    Json,
}

/// What the file listing is ordered by. Plain comments on the variants keep the help to one line.
#[derive(Clone, Copy, ValueEnum)]
enum SortBy {
    // The path, alphabetically.
    Path,
    // All lines of the file.
    Lines,
    // The file's code lines.
    Code,
    // The role, in the order of the role table; the larger file first within one role.
    Role,
    // The file's own language; the larger file first within one language.
    Language,
}

impl From<SortBy> for report::SortKey {
    fn from(sort: SortBy) -> Self {
        match sort {
            SortBy::Path => report::SortKey::Path,
            SortBy::Lines => report::SortKey::Lines,
            SortBy::Code => report::SortKey::Code,
            SortBy::Role => report::SortKey::Role,
            SortBy::Language => report::SortKey::Language,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Order {
    Asc,
    Desc,
}

fn main() -> ExitCode {
    let colour = std::io::stdout().is_terminal() && !colourless();
    let command = Cli::command().mut_subcommand("fleet", |fleet| fleet.after_help(list_help(colour)));
    let cli = Cli::from_arg_matches(&command.get_matches()).unwrap_or_else(|error| error.exit());
    let result = match cli.command {
        Some(Verb::Fleet { repos, each, forks, private, all, output }) => {
            let include = fleet::Include { forks: forks || all, private: private || all };
            run_fleet(repos, each, include, &output)
        }
        None => run_paths(cli.paths, &cli.output),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("freightscan: {message}");
            ExitCode::from(2)
        }
    }
}

/// Whether the NO_COLOR convention asks for no colour: the variable is set and not empty.
fn colourless() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

fn run_paths(paths: Vec<PathBuf>, output: &Output) -> Result<(), String> {
    let roots =
        if paths.is_empty() { vec![std::env::current_dir().map_err(|error| error.to_string())?] } else { paths };
    let mut targets = Vec::new();
    for root in roots {
        if !root.is_dir() {
            // Options before the verb make it a directory.
            let hint = if root == Path::new("fleet") {
                " (options go behind the verb: freightscan fleet --dense)"
            } else {
                ""
            };
            return Err(format!("no such directory: {}{hint}", root.display()));
        }
        let resolved = std::fs::canonicalize(&root).map(|path| PathBuf::from(plain_path(&path))).unwrap_or(root);
        targets.push((None, resolved));
    }
    let scans = scan_all(&targets);
    emit(&scans, output.options(true), output)
}

fn run_fleet(repos: Option<PathBuf>, each: bool, include: fleet::Include, output: &Output) -> Result<(), String> {
    let lookup = |name: &str| std::env::var(name).ok();
    let named = std::env::var(fleet::VARIABLE).ok().filter(|value| !value.is_empty());
    let path = match (repos, named) {
        (Some(path), _) => path,
        (None, Some(value)) => PathBuf::from(
            fleet::expand(&value, &lookup)
                .ok_or_else(|| format!("{} names a variable that is not set: {value}", fleet::VARIABLE))?,
        ),
        (None, None) => {
            return Err(format!(
                "no repository list.

{}",
                list_help(std::io::stderr().is_terminal() && !colourless())
            ));
        }
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read the repository list {}: {error}", path.display()))?;
    let os = if cfg!(windows) { "windows" } else { std::env::consts::OS };
    let fleet = fleet::read(&text, os, include, lookup)
        .map_err(|error| format!("{} is no repository list: {}", path.display(), error.0))?;
    let (present, missing): (Vec<_>, Vec<_>) =
        fleet.repositories.into_iter().partition(|repository| Path::new(&repository.path).is_dir());
    let targets: Vec<(Option<String>, PathBuf)> =
        present.into_iter().map(|repository| (Some(repository.name), PathBuf::from(repository.path))).collect();
    if targets.is_empty() {
        return Err(format!("none of the repositories in {} is on this machine", path.display()));
    }
    let scans = scan_all(&targets);
    emit(&scans, output.options(each || output.files), output)?;
    let absent: Vec<String> = missing.into_iter().map(|repository| repository.name).chain(fleet.unresolved).collect();
    if !absent.is_empty() {
        eprintln!("not on this machine: {}", absent.join(", "));
    }
    Ok(())
}

/// Count every target, all at once, telling a watching terminal what runs while it runs.
fn scan_all(targets: &[(Option<String>, PathBuf)]) -> Vec<Scan> {
    let watched = std::io::stderr().is_terminal();
    let width = terminal_size::terminal_size_of(std::io::stderr()).map_or(80, |(width, _)| width.0 as usize);
    // The directories being scanned, and how many are done.
    let state = Mutex::new((Vec::<String>::new(), 0usize));
    let tell = |text: &str| {
        if watched {
            let text = if colourless() { display::strip(text) } else { text.to_owned() };
            let mut stderr = std::io::stderr().lock();
            let _ = write!(stderr, "\r\x1b[2K{text}");
            let _ = stderr.flush();
        }
    };
    let label = |name: &Option<String>, root: &Path| name.clone().unwrap_or_else(|| plain_path(root));
    let several = targets.len() > 1;
    // A directory starts, or it is done: the bar follows.
    let report = |name: &str, finished: bool| {
        let mut state = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (running, done) = &mut *state;
        if finished {
            running.retain(|other| other != name);
            *done += 1;
        } else {
            running.push(name.to_owned());
        }
        tell(&display::progress(*done, targets.len(), running, width.saturating_sub(1)));
    };
    match targets {
        [(name, root)] => tell(&format!("freightscan: scanning {} …", label(name, root))),
        _ => tell(&display::progress(0, targets.len(), &[], width.saturating_sub(1))),
    }
    let scans = targets
        .par_iter()
        .map(|(name, root)| {
            let name = label(name, root);
            if several {
                report(&name, false);
            }
            let mut scan = adapters::scan(root, adapters::files(root));
            if several {
                report(&name, true);
            }
            scan.name = name;
            scan
        })
        .collect();
    tell("");
    scans
}

fn emit(scans: &[Scan], options: Options, output: &Output) -> Result<(), String> {
    let previous = match &output.journal {
        Some(journal) => take_snapshot(journal, scans)?,
        None => Previous::new(),
    };
    let stdout = std::io::stdout();
    let format = output.format.unwrap_or(if stdout.is_terminal() { Format::Rich } else { Format::Plain });
    let text = match format {
        Format::Json => format!("{:#}\n", render::document(scans, options, &previous)),
        Format::Plain => render::plain(scans, options, &previous),
        Format::Rich => {
            // COLUMNS wins where it is set, as for every terminal tool; without a terminal 80 columns are assumed.
            let width = std::env::var("COLUMNS")
                .ok()
                .and_then(|columns| columns.parse().ok())
                .or_else(|| terminal_size::terminal_size().map(|(width, _)| width.0 as usize))
                .unwrap_or(80);
            let layout = display::Layout { dense: output.dense, width: width.saturating_sub(1) };
            display::show(&render::sections(scans, options, &previous), layout)
        }
    };
    let choice = if colourless() { anstream::ColorChoice::Never } else { anstream::ColorChoice::Always };
    let mut stream = anstream::AutoStream::new(stdout.lock(), choice);
    stream.write_all(text.as_bytes()).map_err(|error| error.to_string())
}

/// Append this run's records and return the latest earlier record of every scanned root.
fn take_snapshot(journal: &Path, scans: &[Scan]) -> Result<Previous, String> {
    let lines =
        adapters::journal_lines(journal).map_err(|error| format!("cannot read {}: {error}", journal.display()))?;
    let records = snapshot::parse(lines.iter().map(String::as_str));
    let previous: Previous = scans
        .iter()
        .filter_map(|scan| {
            snapshot::latest(&records, &scan.root_text()).map(|record| (scan.root_text(), record.clone()))
        })
        .collect();
    let local = jiff::tz::TimeZone::system();
    let zone = clock::zone(adapters::read_config().as_deref(), local);
    let at = jiff::Timestamp::now().to_zoned(zone);
    let records: Vec<_> = scans.iter().map(|scan| snapshot::record(scan, &at)).collect();
    adapters::append_journal(journal, &records)
        .map_err(|error| format!("cannot write {}: {error}", journal.display()))?;
    Ok(previous)
}

/// What a fleet run without a list answers, and what `fleet --help` ends with: the file to write.
fn list_help(colour: bool) -> String {
    let bold = |text: &str| if colour { format!("\x1b[1m{text}\x1b[0m") } else { text.to_owned() };
    let example: Vec<String> = fleet::EXAMPLE
        .lines()
        .map(|line| format!("    {}", if colour { paint_json(line) } else { line.to_owned() }))
        .collect();
    format!(
        "{} names a JSON file of repositories; --repos FILE overrides it. Paths may start with ~ and name\n\
         $env:VARIABLE, the variable's own value too; a repository marked inactive is skipped,\n\
         one marked \"owned\": false unless --forks, one marked \"professional\": false unless --private\n\
         asks for it; --all asks for both.\n\n{}",
        bold(fleet::VARIABLE),
        example.join("\n")
    )
}

/// Colour one line of JSON: keys, string values, literals and punctuation each in their own colour.
fn paint_json(line: &str) -> String {
    const KEY: &str = "\x1b[36m";
    const VALUE: &str = "\x1b[32m";
    const LITERAL: &str = "\x1b[33m";
    const PUNCTUATION: &str = "\x1b[90m";
    const RESET: &str = "\x1b[0m";
    let characters: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character == '"' {
            let end = characters[index + 1..]
                .iter()
                .position(|next| *next == '"')
                .map_or(characters.len() - 1, |at| index + 1 + at);
            let text: String = characters[index..=end].iter().collect();
            let is_key = characters[end + 1..].iter().find(|next| !next.is_whitespace()) == Some(&':');
            out.push_str(&format!("{}{text}{RESET}", if is_key { KEY } else { VALUE }));
            index = end + 1;
        } else if character.is_ascii_alphabetic() {
            let word: String = characters[index..].iter().take_while(|next| next.is_ascii_alphabetic()).collect();
            index += word.len();
            out.push_str(&format!("{LITERAL}{word}{RESET}"));
        } else if "{}[]:,".contains(character) {
            out.push_str(&format!("{PUNCTUATION}{character}{RESET}"));
            index += 1;
        } else {
            out.push(character);
            index += 1;
        }
    }
    out
}
