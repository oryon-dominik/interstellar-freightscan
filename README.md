# freightscan

[![crates.io](https://img.shields.io/crates/v/freightscan.svg)](https://crates.io/crates/freightscan)
[![Test](https://github.com/oryon-dominik/interstellar-freightscan/actions/workflows/test.yml/badge.svg?branch=trunk)](https://github.com/oryon-dominik/interstellar-freightscan/actions/workflows/test.yml)
[![License: MIT](https://img.shields.io/crates/l/freightscan.svg)](LICENSE)

Counts every line of a repository by language, role and kind.

Good line counters exist — [tokei](https://github.com/XAMPPRocky/tokei),
[scc](https://github.com/boyter/scc), [cloc](https://github.com/AlDanial/cloc) — but they count
code, comments and blanks per language. I wanted to know what the lines are for, and I did not
need many languages for it, only the ones my repositories are written in.

freightscan says what the lines of a repository are for — source, test, documentation,
configuration, data, template, script — and what each line is: code, docstring, comment, prose,
data or blank. Records, logs and datasets count as data, not code: JSON Lines, CSV and log files,
and JSON under a `data`, `events`, `logs`, `records`, `reports` or `state` directory.

The name is my soft spot for space fluff: freight is what a starship carries, and freightscan
scans the cargo hold of a repository. 🚀

## Languages

freightscan knows the languages it was built for: Python, Rust, JavaScript, TypeScript, HTML,
CSS, Django templates, Jinja, Shell, PowerShell, Batch, SQL, HCL, Dockerfile, Lua, AutoHotkey,
Emacs Lisp, Mermaid, gettext, Markdown, Jupyter notebooks and plain text, plus TOML, YAML, JSON,
XML, INI, KDL, Caddyfile, justfile, env, git and Windows registry files.

**Not supported at this time:** Go, C, C++, Java, Ruby, PHP, C#, Kotlin, Swift, Dart, Zig, SCSS,
Vue, Svelte and Makefiles. Their files, like every other file freightscan does not know, count as
`unknown`: lines are counted, but not told apart as code or comment.

Python is tokenized. Every other language is read by its comment markers, without reading its
strings: a comment marker inside a string can miscount a line.

Test code is what lies in a `test`, `tests` or `__tests__` directory, `test_*.py`, `*_test.py` and
`conftest.py`, `tests.rs`, `*_test.rs`, `*_tests.rs` and what Rust gates behind `#[cfg(test)]`,
and `*.test.*` and `*.spec.*` in JavaScript and TypeScript.

## Usage

```shell
freightscan                          # the current directory
freightscan ../backend ../frontend  # several directories, each and combined
freightscan --dense                  # compact numbers, no frame — for half a screen
freightscan --format json            # the tables as one JSON document
freightscan --matrix --files         # add language × role, and every file
freightscan --files --sort lines     # every file, largest first; --order asc turns it round
freightscan --detailed               # every kind, role and language apart instead of folded
freightscan --snapshot freight.jsonl # append a record per directory, report the change since the last
freightscan fleet                    # every repository of the list in FREIGHTSCAN_REPOS, one row each
freightscan fleet --tests            # the test share beside every repository
freightscan fleet --forks            # the forks too; --private the private repositories, --all both
```

Inside a git repository freightscan counts what git tracks (git 2.38 or later, for the sizes in
`ls-files --format`). Elsewhere it walks the directory and skips dependency, cache and build
directories. Binaries, lock files, minified assets and files that are not UTF-8 are left out
and reported per reason.

Every section opens with its head: a row per role group — engineering, documentation,
data, other — with its share of all lines, then the lines per kind group — code, docs (docstrings,
comments and prose), data, blank, other — and the test share: test code against test and source
code together. The tables below fold the same way and name the eight largest languages, the rest
as `others`; `--detailed` shows every kind, role and language apart. `NO_COLOR` turns the
colours off. On a terminal the view fits the terminal's width: when the full view is too wide
it gives way step by step — a shorter bar, the frame, the percentages, then the full numbers,
and the bar last. Redirected, the output is plain text.

## Several repositories

`freightscan fleet` reads a JSON list of repositories from the file `FREIGHTSCAN_REPOS` names,
or from `--repos FILE`. Paths may start with `~/` and name environment variables as
`$env:NAME` or `$NAME` — in the list and in `FREIGHTSCAN_REPOS` itself. A repository marked
`inactive` is skipped. So is one that is not our own engineering — a fork marked
`"owned": false`, a private repository marked `"professional": false` — unless `--forks` or
`--private` asks for it, or `--all` for both; both keys default to true. One without a path for
this system, or missing from the disk, is named at the end. While the fleet runs, a progress bar
names the repositories still being scanned.

```json
{
  "repositories": [
    {"name": "atlas", "paths": {"windows": "$env:USERPROFILE/projects/atlas", "posix": "~/projects/atlas"}},
    {"name": "upstream", "paths": {"posix": "~/forks/upstream"}, "owned": false},
    {"name": "diary", "paths": {"posix": "~/diary"}, "professional": false},
    {"name": "relic", "paths": {"windows": "$env:USERPROFILE/projects/relic"}, "inactive": true}
  ]
}
```

## Snapshots and the clock

Every snapshot record carries its time with its UTC offset, and records are compared by
instant, never by their text. The zone comes from `config.toml` in freightscan's configuration
directory (`%APPDATA%/freightscan` on Windows, `$XDG_CONFIG_HOME/freightscan` or
`~/.config/freightscan` elsewhere, or the file `FREIGHTSCAN_CONFIG` names):

```toml
[chronometer]
timezone = "Europe/Berlin"
```

The file may be a symbolic link to a clock file of the same shape that other tools share.
Without a readable zone, freightscan stamps in the local clock's zone.

## Layout

- `src/` — the counting core: pure functions over text and paths; `main.rs` is the command line
- `src/adapters.rs` — git, the file system, the configuration and snapshot files
- `tests/cli.rs` — the command line end to end; unit tests sit beside their module.
  Tests are named for the behaviour they prove.

## Working on it

```shell
just install          # hooks
just test
just lint
just deploy           # cargo install this build
```
