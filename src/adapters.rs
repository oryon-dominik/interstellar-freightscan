//! The edges: git, the file system, the configuration file and the snapshot file.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use rayon::prelude::*;

use crate::types::{Counted, Exclusion, Listed, Scan};
use crate::{count, exclusions};

const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
];

/// List the files below `root`, relative to it: what git tracks, or every file outside skipped directories.
pub fn files(root: &Path) -> Vec<Listed> {
    tracked(root).unwrap_or_else(|| walk(root))
}

/// Ask git for the tracked files below `root`; None when `root` is not inside a repository.
///
/// The index carries every blob's size, so the listing costs no file system call per file.
fn tracked(root: &Path) -> Option<Vec<Listed>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--format=%(objectsize)%x09%(path)"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let listed = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let text = std::str::from_utf8(entry).ok()?;
            let (size, path) = text.split_once('\t')?;
            Some(Listed { path: path.to_owned(), size: size.parse().unwrap_or(0) })
        })
        .collect();
    Some(listed)
}

/// Walk the file system below `root`, skipping dependency, cache and build directories.
fn walk(root: &Path) -> Vec<Listed> {
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, relative)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => {
                    if !SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                        pending.push((entry.path(), path));
                    }
                }
                Ok(kind) if kind.is_file() => {
                    found.push(Listed { path, size: entry.metadata().map(|meta| meta.len()).unwrap_or(0) })
                }
                _ => {}
            }
        }
    }
    found
}

/// Read and count every listed file below `root` on all cores; a file that cannot be read is skipped.
pub fn scan(root: &Path, mut listed: Vec<Listed>) -> Scan {
    // Largest first, so the biggest files do not trail behind everything else.
    listed.sort_by_key(|entry| std::cmp::Reverse(entry.size));
    let results: Vec<Counted> = listed.par_iter().filter_map(|entry| read_and_count(root, &entry.path)).collect();
    count::assemble(root.to_path_buf(), results)
}

/// Count one file; a binary is recognised from its first bytes and never read whole.
fn read_and_count(root: &Path, path: &str) -> Option<Counted> {
    if let Some(reason) = exclusions::by_name(path) {
        return Some(Counted::Excluded(reason));
    }
    let mut file = fs::File::open(root.join(path)).ok()?;
    let mut data = Vec::new();
    (&mut file).take(exclusions::BINARY_PROBE as u64).read_to_end(&mut data).ok()?;
    if exclusions::is_binary(&data) {
        return Some(Counted::Excluded(Exclusion::Binary));
    }
    file.read_to_end(&mut data).ok()?;
    Some(count::count_file(path, &data))
}

/// `FREIGHTSCAN_CONFIG` when set, else `config.toml` in the platform's configuration directory.
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FREIGHTSCAN_CONFIG").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    Some(base?.join("freightscan").join("config.toml"))
}

/// The configuration text, or None when there is no file or a link points nowhere.
pub fn read_config() -> Option<String> {
    fs::read_to_string(config_path()?).ok()
}

/// The lines of the snapshot file, none when it does not exist yet.
pub fn journal_lines(path: &Path) -> std::io::Result<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Append one line per record, creating the file and its directory when needed.
pub fn append_journal(path: &Path, records: &[serde_json::Value]) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut journal = fs::OpenOptions::new().create(true).append(true).open(path)?;
    for record in records {
        writeln!(journal, "{record}")?;
    }
    Ok(())
}
