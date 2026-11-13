//! The repository list `freightscan fleet` reads: a JSON manifest of named repositories with a
//! path per operating system.
//!
//! ```json
//! {"repositories": [{"name": "atlas", "paths": {"windows": "$env:PROJECTS/atlas", "posix": "~/projects/atlas"}}]}
//! ```
//!
//! A path may start with `~/` or name environment variables as `$env:NAME` or `$NAME`. A
//! repository marked `"inactive": true` is skipped. So is one that is not our own engineering —
//! marked `"owned": false` (a fork) or `"professional": false` (private) — unless it is asked for.

use serde_json::Value;

/// A repository list, as `fleet --help` shows it.
pub const EXAMPLE: &str = r#"{
  "repositories": [
    {"name": "atlas", "paths": {"windows": "$env:USERPROFILE/projects/atlas", "posix": "~/projects/atlas"}},
    {"name": "beacon", "paths": {"posix": "~/projects/beacon"}},
    {"name": "upstream", "paths": {"posix": "~/forks/upstream"}, "owned": false},
    {"name": "diary", "paths": {"posix": "~/diary"}, "professional": false},
    {"name": "relic", "paths": {"windows": "$env:USERPROFILE/projects/relic"}, "inactive": true}
  ]
}"#;

/// The environment variable that names the repository list.
pub const VARIABLE: &str = "FREIGHTSCAN_REPOS";

/// One repository of the list, its path resolved for this machine.
#[derive(Debug, PartialEq, Eq)]
pub struct Repository {
    pub name: String,
    pub path: String,
}

/// What the list holds for this machine.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Fleet {
    pub repositories: Vec<Repository>,
    /// Repositories without a path for this operating system, or naming a variable that is not set.
    pub unresolved: Vec<String>,
}

/// The manifest is no JSON object with a `repositories` list.
#[derive(Debug, PartialEq, Eq)]
pub struct Malformed(pub String);

/// What joins the fleet beyond our own professional engineering.
#[derive(Clone, Copy, Debug, Default)]
pub struct Include {
    pub forks: bool,
    pub private: bool,
}

/// Read the manifest for the operating system `os` ("windows", else "posix", "linux" or "macos"),
/// with the forks and private repositories `include` asks for.
pub fn read(
    text: &str,
    os: &str,
    include: Include,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Fleet, Malformed> {
    let document: Value = serde_json::from_str(text).map_err(|error| Malformed(error.to_string()))?;
    let entries = document
        .get("repositories")
        .and_then(Value::as_array)
        .ok_or_else(|| Malformed("no \"repositories\" list".to_owned()))?;
    let keys: &[&str] = if os == "windows" { &["windows"] } else { &[os, "posix"] };
    let mut fleet = Fleet::default();
    for entry in entries {
        let flag = |key: &str| entry.get(key).and_then(Value::as_bool);
        let fork = flag("owned") == Some(false);
        let private = flag("professional") == Some(false);
        if flag("inactive") == Some(true) || (fork && !include.forks) || (private && !include.private) {
            continue;
        }
        let Some(name) = entry.get("name").and_then(Value::as_str) else { continue };
        let raw = keys.iter().find_map(|key| entry.get("paths")?.get(key)?.as_str());
        match raw.and_then(|raw| expand(raw, &lookup)) {
            Some(path) => fleet.repositories.push(Repository { name: name.to_owned(), path }),
            None => fleet.unresolved.push(name.to_owned()),
        }
    }
    Ok(fleet)
}

/// Replace `~` and environment variables in a path; None when a variable is not set.
///
/// Only `~` alone or before `/` is the home directory; `~name` stays as written.
pub fn expand(raw: &str, lookup: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = raw;
    if let Some(after) = rest.strip_prefix('~').filter(|after| after.is_empty() || after.starts_with('/')) {
        out.push_str(&lookup("HOME").or_else(|| lookup("USERPROFILE"))?);
        rest = after;
    }
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let after = after.strip_prefix("env:").unwrap_or(after);
        let end = after
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(after.len());
        out.push_str(&lookup(&after[..end])?);
        rest = &after[end..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "PROJECTS" => Some("C:/Users/ada/projects".to_owned()),
            "HOME" => Some("/home/ada".to_owned()),
            _ => None,
        }
    }

    const MANIFEST: &str = r#"{"repositories": [
        {"name": "atlas", "paths": {"windows": "$env:PROJECTS/atlas", "posix": "~/projects/atlas"}},
        {"name": "beacon", "paths": {"windows": "$env:PROJECTS/beacon"}},
        {"name": "retired", "paths": {"windows": "c:/dev/retired"}, "inactive": true},
        {"name": "upstream", "paths": {"windows": "c:/forks/upstream"}, "owned": false},
        {"name": "diary", "paths": {"windows": "c:/diary"}, "professional": false},
        {"name": "elsewhere", "paths": {"windows": "$env:NOWHERE/elsewhere"}}
    ]}"#;

    #[test]
    fn the_fleet_reads_every_active_repository_for_this_system() {
        let fleet = read(MANIFEST, "windows", Include::default(), lookup).unwrap();
        let paths: Vec<_> = fleet.repositories.iter().map(|repository| repository.path.as_str()).collect();
        assert_eq!(paths, ["C:/Users/ada/projects/atlas", "C:/Users/ada/projects/beacon"]);
        assert_eq!(fleet.unresolved, ["elsewhere"]);
    }

    #[test]
    fn a_posix_machine_reads_the_posix_paths() {
        let fleet = read(MANIFEST, "linux", Include::default(), lookup).unwrap();
        assert_eq!(
            fleet.repositories,
            [Repository { name: "atlas".to_owned(), path: "/home/ada/projects/atlas".to_owned() }]
        );
        assert_eq!(fleet.unresolved, ["beacon", "elsewhere"]);
    }

    #[test]
    fn forks_and_private_repositories_join_only_when_asked_for() {
        let named = |forks, private| -> Vec<String> {
            let fleet = read(MANIFEST, "windows", Include { forks, private }, lookup).unwrap();
            fleet.repositories.into_iter().map(|repository| repository.name).collect()
        };
        assert_eq!(named(false, false), ["atlas", "beacon"]);
        assert_eq!(named(true, false), ["atlas", "beacon", "upstream"]);
        assert_eq!(named(false, true), ["atlas", "beacon", "diary"]);
        assert_eq!(named(true, true), ["atlas", "beacon", "upstream", "diary"]);
    }

    #[test]
    fn the_example_is_a_list_freightscan_reads() {
        let fleet = read(EXAMPLE, "linux", Include { forks: true, private: true }, lookup).unwrap();
        assert_eq!(fleet.repositories.len(), 4);
    }

    #[test]
    fn only_a_tilde_alone_or_before_a_slash_is_the_home_directory() {
        assert_eq!(expand("~", &lookup).as_deref(), Some("/home/ada"));
        assert_eq!(expand("~/x", &lookup).as_deref(), Some("/home/ada/x"));
        assert_eq!(expand("~alice/x", &lookup).as_deref(), Some("~alice/x"));
    }

    #[test]
    fn a_manifest_without_a_repository_list_is_malformed() {
        assert!(read("{}", "windows", Include::default(), lookup).is_err());
        assert!(read("not json", "windows", Include::default(), lookup).is_err());
    }
}
