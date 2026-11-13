//! The command line end to end: discovery, counting, the outputs, snapshots and the fleet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

const PROBE: &[u8] = b"\"\"\"Probe.\"\"\"\n\nvalue = 1\n";
const TOKYO: &str = "[chronometer]\ntimezone = \"Asia/Tokyo\"\n";

/// A scratch directory that builds repositories below it and runs freightscan against them.
struct Scratch {
    directory: TempDir,
    count: std::cell::Cell<usize>,
}

impl Scratch {
    fn new() -> Self {
        Self { directory: TempDir::new().unwrap(), count: std::cell::Cell::new(0) }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn write(root: &Path, files: &[(&str, &[u8])]) {
        for (name, data) in files {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }
    }

    /// A git repository whose index tracks exactly the files handed in.
    fn git_repository(&self, files: &[(&str, &[u8])]) -> PathBuf {
        let root = self.next("git");
        git(&["init", "--quiet"], &root);
        Self::write(&root, files);
        git(&["add", "--all"], &root);
        root
    }

    /// A directory outside any git repository.
    fn plain_directory(&self, files: &[(&str, &[u8])]) -> PathBuf {
        let root = self.next("plain");
        Self::write(&root, files);
        root
    }

    fn next(&self, prefix: &str) -> PathBuf {
        self.count.set(self.count.get() + 1);
        let root = self.path(&format!("{prefix}-{}", self.count.get()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// Run freightscan with a configuration file in this scratch, never the machine's own.
    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_freightscan"))
            .args(arguments)
            .env("FREIGHTSCAN_CONFIG", self.path("config.toml"))
            .env_remove("FREIGHTSCAN_REPOS")
            .output()
            .unwrap()
    }

    fn stdout(&self, arguments: &[&str]) -> String {
        let output = self.run(arguments);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let mut arguments = arguments.to_vec();
        arguments.extend(["--format", "json"]);
        serde_json::from_str(&self.stdout(&arguments)).unwrap()
    }
}

fn git(arguments: &[&str], root: &Path) {
    let status = Command::new("git").arg("-C").arg(root).args(arguments).status().unwrap();
    assert!(status.success());
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

#[test]
fn inside_git_only_tracked_files_are_counted() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", PROBE)]);
    fs::write(root.join("untracked.py"), "value = 2\n").unwrap();
    assert_eq!(scratch.json(&[&text(&root)])["scans"][0]["total"], json!({"code": 1, "docstring": 1, "blank": 1}));
}

#[test]
fn outside_git_dependency_cache_and_build_directories_are_skipped() {
    let scratch = Scratch::new();
    let root = scratch.plain_directory(&[
        ("probe.py", PROBE),
        (".venv/lib/site.py", b"value = 1\n"),
        ("node_modules/package/index.js", b"value = 1;\n"),
        ("target/debug/build.rs", b"fn main() {}\n"),
        ("src/__pycache__/probe.py", b"value = 1\n"),
    ]);
    assert_eq!(scratch.json(&[&text(&root)])["scans"][0]["total"], json!({"code": 1, "docstring": 1, "blank": 1}));
}

#[test]
fn the_exclusions_are_reported_per_reason() {
    let scratch = Scratch::new();
    let root =
        scratch.git_repository(&[("probe.py", PROBE), ("django.mo", b"\x00\x01"), ("uv.lock", b"version = 1\n")]);
    assert_eq!(scratch.json(&[&text(&root)])["scans"][0]["excluded"], json!({"binary": 1, "lock": 1}));
    assert!(scratch.stdout(&[&text(&root), "--format", "plain"]).contains("excluded: binary 1, lock 1"));
}

#[test]
fn a_missing_directory_ends_with_an_error_that_names_it() {
    let scratch = Scratch::new();
    let output = scratch.run(&[&text(&scratch.path("nowhere"))]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("nowhere"));
}

#[test]
fn unparsed_python_files_are_reported() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("broken.py", b"def broken(:\n")]);
    assert_eq!(scratch.json(&[&text(&root)])["scans"][0]["unparsed"], json!(["broken.py"]));
    assert!(scratch.stdout(&[&text(&root), "--format", "plain"]).contains("unparsed: broken.py"));
}

#[test]
fn output_that_is_not_a_terminal_carries_no_escape_sequences() {
    let scratch = Scratch::new();
    let output = scratch.stdout(&[&text(&scratch.git_repository(&[("probe.py", PROBE)]))]);
    assert!(!output.contains('\x1b'));
    assert!(output.contains("Python"));
}

#[test]
fn the_json_document_carries_the_tables_and_a_schema_version() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", PROBE), ("tests/test_probe.py", b"assert True\n")]);
    let document = scratch.json(&[&text(&root)]);
    assert_eq!(document["schema"], 3);
    let repository = &document["scans"][0];
    assert_eq!(repository["languages"], json!({"Python": {"code": 2, "docstring": 1, "blank": 1}}));
    assert_eq!(repository["roles"], json!({"source": {"code": 1, "docstring": 1, "blank": 1}, "test": {"code": 1}}));
}

#[test]
fn several_directories_are_reported_each_and_combined() {
    let scratch = Scratch::new();
    let first = scratch.git_repository(&[("probe.py", PROBE)]);
    let second = scratch.git_repository(&[("other.py", b"value = 2\n")]);
    let document = scratch.json(&[&text(&first), &text(&second)]);
    let totals: Vec<&Value> = document["scans"].as_array().unwrap().iter().map(|scan| &scan["total"]).collect();
    assert_eq!(totals, [&json!({"code": 1, "docstring": 1, "blank": 1}), &json!({"code": 1})]);
    assert_eq!(document["combined"]["total"], json!({"code": 2, "docstring": 1, "blank": 1}));
    let plain = scratch.stdout(&[&text(&first), &text(&second), "--format", "plain"]);
    assert!(plain.contains("all 2 directories"));
}

#[test]
fn and_26_the_terminal_view_has_a_head_and_a_language_table_and_details_on_demand() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[
        ("probe.py", PROBE),
        ("tests/test_probe.py", b"assert True\n"),
        ("Cargo.toml", b"[package]\n"),
    ]);
    let folded = strip(&scratch.stdout(&[&text(&root), "--format", "rich"]));
    assert!(folded.contains("engineering") && folded.contains("tests 50.0%"), "{folded}");
    let language_table = &folded[folded.find("language").unwrap()..];
    assert!(language_table.contains("Python") && language_table.contains("TOML"));
    assert!(language_table.contains("docs") && !language_table.contains("docstring"), "{language_table}");
    let detailed = strip(&scratch.stdout(&[&text(&root), "--format", "rich", "--detailed"]));
    let role_table = &detailed[detailed.find("\n│ role").unwrap()..detailed.find("\n│ language").unwrap()];
    for row in ["source", "test", "docstring"] {
        assert!(role_table.contains(row), "{row}");
    }
}

#[test]
fn the_file_listing_reports_every_file() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", PROBE), ("tests/test_probe.py", b"assert True\n")]);
    let files = &scratch.json(&[&text(&root), "--files"])["scans"][0]["files"];
    assert_eq!(
        *files,
        json!([
            {"path": "probe.py", "role": "source", "cells": [["Python", "source", "code", 1], ["Python", "source", "docstring", 1], ["Python", "source", "blank", 1]]},
            {"path": "tests/test_probe.py", "role": "test", "cells": [["Python", "test", "code", 1]]},
        ])
    );
    assert!(scratch.stdout(&[&text(&root), "--files", "--format", "plain"]).contains("tests/test_probe.py"));
}

#[test]
fn the_matrix_reports_every_count_per_language_role_and_kind() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", PROBE), ("tests/test_probe.py", b"assert True\n")]);
    let matrix = &scratch.json(&[&text(&root), "--matrix"])["scans"][0]["matrix"];
    assert_eq!(
        *matrix,
        json!([
            ["Python", "source", "code", 1],
            ["Python", "source", "docstring", 1],
            ["Python", "source", "blank", 1],
            ["Python", "test", "code", 1],
        ])
    );
    assert!(scratch.stdout(&[&text(&root), "--matrix", "--format", "plain"]).contains("Python · test"));
}

#[test]
fn without_the_switches_the_json_carries_neither_matrix_nor_files() {
    let scratch = Scratch::new();
    let repository = &scratch.json(&[&text(&scratch.git_repository(&[("probe.py", PROBE)]))])["scans"][0];
    assert!(repository.get("matrix").is_none() && repository.get("files").is_none());
}

#[test]
fn and_42_the_dense_view_fits_the_width_it_is_given() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[
        ("probe.py", PROBE),
        ("documentation/guide.md", b"# Guide\n\nWords.\n```\nraw\n```\n"),
        ("tests/test_probe.py", b"assert True\n"),
        ("install/settings.json", b"{}\n"),
    ]);
    let output = Command::new(env!("CARGO_BIN_EXE_freightscan"))
        .args([&text(&root), "--format", "rich", "--dense"])
        .env("FREIGHTSCAN_CONFIG", scratch.path("config.toml"))
        .env_remove("COLUMNS")
        .output()
        .unwrap();
    let shown = strip(&String::from_utf8(output.stdout).unwrap());
    // Without a terminal the width falls back to 80 columns, one kept free.
    let widest = shown.lines().map(|line| line.chars().count()).max().unwrap();
    assert!(widest <= 79, "{widest} columns:\n{shown}");
    assert!(!shown.contains('╭'));
}

#[test]
fn the_largest_row_fills_its_bar_and_the_share_is_a_percentage() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", PROBE), ("tests/test_probe.py", b"assert True\n")]);
    let output = Command::new(env!("CARGO_BIN_EXE_freightscan"))
        .args([&text(&root), "--format", "rich", "--detailed"])
        .env("FREIGHTSCAN_CONFIG", scratch.path("config.toml"))
        .env("COLUMNS", "200")
        .output()
        .unwrap();
    let shown = strip(&String::from_utf8(output.stdout).unwrap());
    let source = shown.lines().find(|line| line.trim_start_matches(['│', ' ']).starts_with("source")).unwrap();
    assert!(source.contains(&"█".repeat(24)), "{source}");
    assert!(source.contains("75.0%"), "{source}");
}

fn strip(text: &str) -> String {
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

// -- snapshots

fn snap(scratch: &Scratch, root: &Path, journal: &Path) -> Vec<Value> {
    scratch.json(&[&text(root), "--snapshot", &text(journal)]);
    fs::read_to_string(journal).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

/// The UTC offset a record was stamped with, in seconds, read from the stamp's `+hh:mm` tail.
fn offset_seconds(record: &Value) -> i32 {
    let stamp = record["at"].as_str().unwrap();
    let offset = &stamp[stamp.len() - 6..];
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let hours: i32 = offset[1..3].parse().unwrap();
    let minutes: i32 = offset[4..6].parse().unwrap();
    sign * (hours * 3600 + minutes * 60)
}

#[test]
fn each_run_appends_one_line_per_directory() {
    let scratch = Scratch::new();
    let first = scratch.git_repository(&[("probe.py", b"value = 1\n")]);
    let second = scratch.git_repository(&[("other.py", b"value = 2\n")]);
    let journal = scratch.path("freight.jsonl");
    snap(&scratch, &first, &journal);
    assert_eq!(snap(&scratch, &first, &journal).len(), 2);
    scratch.json(&[&text(&first), &text(&second), "--snapshot", &text(&journal)]);
    assert_eq!(fs::read_to_string(&journal).unwrap().lines().count(), 4);
}

#[test]
fn every_record_carries_its_utc_offset() {
    let scratch = Scratch::new();
    let record =
        &snap(&scratch, &scratch.git_repository(&[("probe.py", b"value = 1\n")]), &scratch.path("freight.jsonl"))[0];
    // Without a configured zone the local clock stamps, with its offset.
    assert_eq!(offset_seconds(record), jiff::Zoned::now().offset().seconds());
    assert_eq!(record["matrix"], json!([["Python", "source", "code", 1]]));
}

#[test]
fn a_configured_zone_stamps_the_records() {
    let scratch = Scratch::new();
    fs::write(scratch.path("config.toml"), TOKYO).unwrap();
    let record =
        &snap(&scratch, &scratch.git_repository(&[("probe.py", b"value = 1\n")]), &scratch.path("freight.jsonl"))[0];
    assert_eq!(offset_seconds(record), 9 * 3600);
}

#[test]
fn a_configuration_linked_to_a_chronometer_reads_its_zone() {
    let scratch = Scratch::new();
    let chronometer = scratch.path("chronometer.toml");
    fs::write(&chronometer, TOKYO).unwrap();
    if !link(&chronometer, &scratch.path("config.toml")) {
        eprintln!("skipped: this machine refuses symbolic links");
        return;
    }
    let record =
        &snap(&scratch, &scratch.git_repository(&[("probe.py", b"value = 1\n")]), &scratch.path("freight.jsonl"))[0];
    assert_eq!(offset_seconds(record), 9 * 3600);
}

#[test]
fn a_dangling_link_falls_back_to_the_local_clock() {
    let scratch = Scratch::new();
    if !link(&scratch.path("gone.toml"), &scratch.path("config.toml")) {
        eprintln!("skipped: this machine refuses symbolic links");
        return;
    }
    let record =
        &snap(&scratch, &scratch.git_repository(&[("probe.py", b"value = 1\n")]), &scratch.path("freight.jsonl"))[0];
    let local = jiff::Zoned::now().offset().seconds();
    assert_eq!(offset_seconds(record), local);
}

fn link(source: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(source, target);
    #[cfg(not(windows))]
    let linked = std::os::unix::fs::symlink(source, target);
    linked.is_ok()
}

#[test]
fn the_change_per_role_is_reported_against_the_latest_record() {
    let scratch = Scratch::new();
    let root =
        scratch.git_repository(&[("probe.py", b"value = 1\nvalue = 2\n"), ("tests/test_probe.py", b"assert True\n")]);
    let canonical = fs::canonicalize(&root).unwrap();
    let root_text = canonical.to_string_lossy().replace('\\', "/");
    let root_text = root_text.strip_prefix("//?/").unwrap_or(&root_text);
    let journal = scratch.path("freight.jsonl");
    let earlier = json!({
        "schema": 1,
        "root": root_text,
        "at": "2026-10-01T09:00:00+02:00",
        "matrix": [["Python", "source", "code", 1], ["Python", "test", "code", 3]],
        "excluded": {},
    });
    fs::write(&journal, format!("{earlier}\n")).unwrap();
    let document = scratch.json(&[&text(&root), "--snapshot", &text(&journal)]);
    assert_eq!(
        document["scans"][0]["since"],
        json!({"at": "2026-10-01T09:00:00+02:00", "roles": {"source": 1, "test": -2}})
    );
    let plain = scratch.stdout(&[&text(&root), "--snapshot", &text(&journal), "--format", "plain"]);
    assert!(plain.contains("since ") && plain.contains("source +0"), "{plain}");
}

// -- the fleet

fn manifest(scratch: &Scratch, entries: Value) -> PathBuf {
    let path = scratch.path("repositories.json");
    fs::write(&path, json!({"repositories": entries}).to_string()).unwrap();
    path
}

fn os_key() -> &'static str {
    if cfg!(windows) { "windows" } else { "posix" }
}

#[test]
fn and_44_the_fleet_scans_every_repository_as_one_row_and_names_the_absent() {
    let scratch = Scratch::new();
    let alpha = scratch.git_repository(&[("probe.py", PROBE)]);
    let beta = scratch.git_repository(&[("other.py", b"value = 2\n")]);
    let list = manifest(
        &scratch,
        json!([
            {"name": "alpha", "paths": {os_key(): text(&alpha)}},
            {"name": "beta", "paths": {os_key(): text(&beta)}},
            {"name": "retired", "paths": {os_key(): text(&scratch.path("retired"))}, "inactive": true},
            {"name": "diary", "paths": {os_key(): text(&scratch.path("diary"))}, "professional": false},
            {"name": "faraway", "paths": {os_key(): text(&scratch.path("faraway"))}},
        ]),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_freightscan"))
        .args(["fleet", "--format", "plain"])
        .env("FREIGHTSCAN_REPOS", &list)
        .env("FREIGHTSCAN_CONFIG", scratch.path("config.toml"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let plain = String::from_utf8(output.stdout).unwrap();
    assert!(plain.contains("all 2 directories"), "{plain}");
    let directory_table = &plain[plain.find("directory").unwrap()..plain.find("\nlanguage").unwrap()];
    assert!(directory_table.contains("alpha") && directory_table.contains("beta"), "{directory_table}");
    assert!(!plain.contains(&text(&alpha).replace('\\', "/")), "each repository gets a section only with --each");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not on this machine: faraway") && !stderr.contains("retired"), "{stderr}");
    assert!(!stderr.contains("diary"), "{stderr}");
}

#[test]
fn forks_and_private_repositories_join_the_fleet_only_when_asked_for() {
    let scratch = Scratch::new();
    let work = scratch.git_repository(&[("probe.py", PROBE)]);
    let diary = scratch.git_repository(&[("notes.py", b"value = 2\n")]);
    let upstream = scratch.git_repository(&[("fork.py", b"value = 3\n")]);
    let list = manifest(
        &scratch,
        json!([
            {"name": "work", "paths": {os_key(): text(&work)}},
            {"name": "diary", "paths": {os_key(): text(&diary)}, "professional": false},
            {"name": "upstream", "paths": {os_key(): text(&upstream)}, "owned": false},
        ]),
    );
    let list = text(&list);
    let scans = |extra: &[&str]| -> usize {
        let args: Vec<&str> = ["fleet", "--repos", list.as_str()].into_iter().chain(extra.iter().copied()).collect();
        scratch.json(&args)["scans"].as_array().unwrap().len()
    };
    assert_eq!(scans(&[]), 1);
    assert_eq!(scans(&["--forks"]), 2);
    assert_eq!(scans(&["--private"]), 2);
    assert_eq!(scans(&["--all"]), 3);
}

#[test]
fn tests_adds_the_test_share_of_every_directory() {
    let scratch = Scratch::new();
    let tested = scratch.git_repository(&[("probe.py", b"value = 1\n"), ("tests/test_probe.py", b"assert True\n")]);
    let untested = scratch.git_repository(&[("other.py", b"value = 2\n")]);
    let list = manifest(
        &scratch,
        json!([
            {"name": "tested", "paths": {os_key(): text(&tested)}},
            {"name": "untested", "paths": {os_key(): text(&untested)}},
        ]),
    );
    let plain = scratch.stdout(&["fleet", "--repos", &text(&list), "--tests", "--format", "plain"]);
    assert!(row_of(&plain, "tested").ends_with("50.0%") && row_of(&plain, "untested").ends_with("0.0%"), "{plain}");
    assert!(row_of(&plain, "total").ends_with("33.3%"), "{plain}");
    let without = scratch.stdout(&["fleet", "--repos", &text(&list), "--format", "plain"]);
    assert!(!row_of(&without, "tested").ends_with('%'), "{without}");
}

fn row_of(text: &str, name: &str) -> String {
    text.lines().find(|line| line.starts_with(name)).unwrap().to_owned()
}

#[test]
fn the_fleet_list_can_be_passed_on_the_command_line() {
    let scratch = Scratch::new();
    let alpha = scratch.git_repository(&[("probe.py", PROBE)]);
    let list = manifest(&scratch, json!([{"name": "alpha", "paths": {os_key(): text(&alpha)}}]));
    let document = scratch.json(&["fleet", "--repos", &text(&list)]);
    assert_eq!(document["scans"][0]["total"], json!({"code": 1, "docstring": 1, "blank": 1}));
}

#[test]
fn the_fleet_lists_files_and_changes_without_each() {
    let scratch = Scratch::new();
    let alpha = scratch.git_repository(&[("probe.py", PROBE)]);
    let beta = scratch.git_repository(&[("other.py", b"value = 2\n")]);
    let list = manifest(
        &scratch,
        json!([
            {"name": "alpha", "paths": {os_key(): text(&alpha)}},
            {"name": "beta", "paths": {os_key(): text(&beta)}},
        ]),
    );
    let list = text(&list);
    let run = |extra: &[&str]| {
        let arguments: Vec<&str> =
            ["fleet", "--repos", list.as_str(), "--format", "plain"].into_iter().chain(extra.iter().copied()).collect();
        scratch.stdout(&arguments)
    };
    assert!(run(&["--files"]).contains("other.py"));
    let journal = text(&scratch.path("freight.jsonl"));
    run(&["--snapshot", &journal]);
    let again = run(&["--snapshot", &journal]);
    assert!(again.contains("alpha: since ") && again.contains("beta: since "), "{again}");
}

#[test]
fn inline_rust_tests_count_as_test_code() {
    let scratch = Scratch::new();
    let source = b"fn main() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn launch() {}\n}\n";
    let root = scratch.git_repository(&[("src/main.rs", source)]);
    assert!(scratch.stdout(&[&text(&root), "--format", "plain"]).contains("tests 83.3%"));
    let file = &scratch.json(&[&text(&root), "--files"])["scans"][0]["files"][0];
    assert_eq!(
        file["cells"],
        json!([["Rust", "source", "code", 1], ["Rust", "source", "blank", 1], ["Rust", "test", "code", 5]])
    );
}

#[test]
fn tests_adds_the_test_share_to_the_json_document() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", b"value = 1\n"), ("tests/test_probe.py", b"assert True\n")]);
    let other = scratch.git_repository(&[("other.py", b"value = 2\n")]);
    let document = scratch.json(&[&text(&root), &text(&other), "--tests"]);
    assert_eq!(document["scans"][0]["tests"], json!({"test": 1, "source": 1, "share": 50.0}));
    assert_eq!(document["combined"]["tests"]["source"], 2);
    assert!(scratch.json(&[&text(&root)])["scans"][0].get("tests").is_none());
}

#[test]
fn no_color_keeps_the_rich_view_free_of_colour() {
    let scratch = Scratch::new();
    let root = scratch.git_repository(&[("probe.py", PROBE)]);
    let run = |value: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_freightscan"))
            .args([&text(&root), "--format", "rich"])
            .env("FREIGHTSCAN_CONFIG", scratch.path("config.toml"))
            .env("NO_COLOR", value)
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    };
    assert!(!run("1").contains('\x1b'));
    assert!(run("").contains('\x1b'));
}

#[test]
fn an_option_before_the_verb_is_told_where_it_belongs() {
    let scratch = Scratch::new();
    let output = scratch.run(&["--dense", "fleet"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("freightscan fleet --dense"));
}

#[test]
fn without_a_repository_list_the_fleet_asks_for_one() {
    let scratch = Scratch::new();
    let output = scratch.run(&["fleet"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no repository list") && stderr.contains("FREIGHTSCAN_REPOS"), "{stderr}");
}
