//! The role table: one standard for every repository, the first matching rule wins.

use crate::languages::{self, Language};
use crate::types::Role;

const TEST_DIRECTORIES: &[&str] = &["test", "tests", "__tests__"];
const TEMPLATE_DIRECTORIES: &[&str] = &["templates"];
const DOCUMENTATION_DIRECTORIES: &[&str] = &["documentation", "docs"];
const SCRIPT_DIRECTORIES: &[&str] = &["scripts"];
const DATA_DIRECTORIES: &[&str] = &["data", "events", "logs", "records", "reports", "state"];

const DOCUMENTATION_LANGUAGES: &[&Language] =
    &[&languages::MARKDOWN, &languages::NOTEBOOK, &languages::TEXT, &languages::MERMAID];
const DATA_LANGUAGES: &[&Language] = &[&languages::CSV, &languages::LOG, &languages::JSON_LINES];
const SCRIPT_LANGUAGES: &[&Language] = &[&languages::SHELL, &languages::POWERSHELL, &languages::BATCH];
const CONFIGURATION_LANGUAGES: &[&Language] = &[
    &languages::TOML,
    &languages::YAML,
    &languages::JUSTFILE,
    &languages::ENV,
    &languages::JSON,
    &languages::JSONC,
    &languages::INI,
    &languages::GIT,
    &languages::REGISTRY,
    &languages::CADDYFILE,
    &languages::KDL,
    &languages::HCL,
    &languages::DOCKERFILE,
];
const HTML_LANGUAGES: &[&Language] = &[&languages::HTML, &languages::DJANGO_TEMPLATE];
const TEMPLATE_LANGUAGES: &[&Language] = &[&languages::DJANGO_TEMPLATE, &languages::JINJA];

/// Return the role of the file at `path`, relative to the scanned directory.
pub fn role(path: &str, language: &Language) -> Role {
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let in_any = |names: &[&str]| parent.split('/').any(|part| names.contains(&part));
    let is = |table: &[&Language]| table.contains(&language);
    if in_any(TEST_DIRECTORIES) || is_test_file(name) {
        Role::Test
    } else if is(TEMPLATE_LANGUAGES)
        // An env template is configuration, whatever its suffix.
        || (languages::unwrap_template(name).is_some() && *language != languages::ENV)
        || (is(HTML_LANGUAGES) && in_any(TEMPLATE_DIRECTORIES))
    {
        Role::Template
    } else if in_any(DOCUMENTATION_DIRECTORIES) || is(DOCUMENTATION_LANGUAGES) {
        Role::Documentation
    } else if in_any(SCRIPT_DIRECTORIES) || is(SCRIPT_LANGUAGES) {
        Role::Script
    } else if is(DATA_LANGUAGES) || (*language == languages::JSON && in_any(DATA_DIRECTORIES)) {
        Role::Data
    } else if is(CONFIGURATION_LANGUAGES) {
        Role::Configuration
    } else if *language == languages::UNKNOWN {
        Role::Other
    } else {
        Role::Source
    }
}

/// Tell whether a file name follows the naming of test files in its language.
fn is_test_file(name: &str) -> bool {
    let (stem, suffix) = name.rsplit_once('.').unwrap_or((name, ""));
    match suffix {
        "py" => stem == "conftest" || stem.starts_with("test_") || stem.ends_with("_test"),
        "rs" => stem == "tests" || stem.ends_with("_test") || stem.ends_with("_tests"),
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => {
            stem.ends_with(".test") || stem.ends_with(".spec")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::language_of;

    fn role_of(path: &str) -> Role {
        role(path, language_of(path))
    }

    #[test]
    fn roles_follow_the_role_table() {
        let table = [
            ("tests/test_count.py", Role::Test),
            ("application/apps/space/tests/factories.py", Role::Test),
            ("test/helpers.py", Role::Test),
            ("src/freightscan/test_inline.py", Role::Test),
            ("src/freightscan/count_test.py", Role::Test),
            ("conftest.py", Role::Test),
            ("src/tests.rs", Role::Test),
            ("src/count_test.rs", Role::Test),
            ("src/count_tests.rs", Role::Test),
            ("web/app.test.ts", Role::Test),
            ("web/app.spec.js", Role::Test),
            ("web/__tests__/app.js", Role::Test),
            ("web/contest.ts", Role::Source),
            ("src/contest.rs", Role::Source),
            ("templates/base.djt.html", Role::Template),
            ("application/templates/components/card.html", Role::Template),
            ("template/pyproject.toml.jinja", Role::Template),
            ("app_template/admin.py-tpl", Role::Template),
            ("documentation/guides/README.md", Role::Documentation),
            ("docs/conf.py", Role::Documentation),
            ("README.md", Role::Documentation),
            ("notebooks/analysis.ipynb", Role::Documentation),
            ("LICENSE", Role::Documentation),
            ("scripts/python/version.py", Role::Script),
            ("install.sh", Role::Script),
            ("bin/launch.cmd", Role::Script),
            ("queue/state/messages.jsonl", Role::Data),
            ("records/daily/2026-10-19.json", Role::Data),
            ("tracker/state/registry.json", Role::Data),
            ("shared/logs/history.log", Role::Data),
            ("datasets/income.csv", Role::Data),
            ("tests/fixtures/records/run.json", Role::Test),
            ("pyproject.toml", Role::Configuration),
            ("infrastructure.tf", Role::Configuration),
            ("compose/Dockerfile", Role::Configuration),
            (".github/workflows/ci.yml", Role::Configuration),
            (".justfile.library", Role::Configuration),
            ("envs/develop.env", Role::Configuration),
            (".env.template", Role::Configuration),
            ("envs/backups.env.template", Role::Configuration),
            ("install/scoops/scoop-packages.json", Role::Configuration),
            (".gitattributes", Role::Configuration),
            ("src/freightscan/count.py", Role::Source),
            ("src/main.rs", Role::Source),
            ("htoprc", Role::Other),
        ];
        for (path, expected) in table {
            assert_eq!(role_of(path), expected, "{path}");
        }
    }

    #[test]
    fn the_first_matching_row_wins() {
        assert_eq!(role("tests/fixtures/run.sh", &languages::SHELL), Role::Test);
    }
}
