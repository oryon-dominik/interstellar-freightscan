//! The values freightscan passes between its core and its adapters.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Code,
    Docstring,
    Comment,
    Prose,
    Data,
    Blank,
    Unclassified,
}

impl Kind {
    pub const ALL: [Kind; 7] =
        [Kind::Code, Kind::Docstring, Kind::Comment, Kind::Prose, Kind::Data, Kind::Blank, Kind::Unclassified];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Code => "code",
            Kind::Docstring => "docstring",
            Kind::Comment => "comment",
            Kind::Prose => "prose",
            Kind::Data => "data",
            Kind::Blank => "blank",
            Kind::Unclassified => "unclassified",
        }
    }
}

/// What a file is for — the first matching rule in `roles` decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Source,
    Test,
    Documentation,
    Configuration,
    Data,
    Template,
    Script,
    Other,
}

impl Role {
    pub const ALL: [Role; 8] = [
        Role::Source,
        Role::Test,
        Role::Documentation,
        Role::Configuration,
        Role::Data,
        Role::Template,
        Role::Script,
        Role::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::Source => "source",
            Role::Test => "test",
            Role::Documentation => "documentation",
            Role::Configuration => "configuration",
            Role::Data => "data",
            Role::Template => "template",
            Role::Script => "script",
            Role::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.name() == name)
    }
}

/// Why a file is left out of the counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Exclusion {
    Binary,
    Lock,
    Minified,
    Undecodable,
}

impl Exclusion {
    pub fn name(self) -> &'static str {
        match self {
            Exclusion::Binary => "binary",
            Exclusion::Lock => "lock",
            Exclusion::Minified => "minified",
            Exclusion::Undecodable => "undecodable",
        }
    }
}

macro_rules! display_by_name {
    ($($kind:ty),*) => {$(
        impl fmt::Display for $kind {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.pad(self.name())
            }
        }
    )*};
}
display_by_name!(Kind, Role, Exclusion);

/// Lines of one language, role and kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub language: &'static str,
    pub role: Role,
    pub kind: Kind,
    pub lines: u64,
}

/// The counted lines of one file; `unparsed` marks Python that fell back to comment markers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCount {
    /// Relative to the scanned directory, with forward slashes.
    pub path: String,
    pub cells: Vec<Cell>,
    pub unparsed: bool,
}

/// Everything counted below one directory; `name` labels it among several.
#[derive(Clone, Debug)]
pub struct Scan {
    pub name: String,
    pub root: PathBuf,
    pub files: Vec<FileCount>,
    pub excluded: BTreeMap<Exclusion, u64>,
}

impl Scan {
    /// The root as freightscan prints it.
    pub fn root_text(&self) -> String {
        plain_path(&self.root)
    }
}

/// A path as freightscan prints it: forward slashes, no verbatim prefix. It must still open:
/// a mapped drive resolves to a network share, verbatim as `\\?\UNC\host\share`.
pub fn plain_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    match text.strip_prefix("//?/") {
        Some(rest) => rest.strip_prefix("UNC/").map_or_else(|| rest.to_owned(), |share| format!("//{share}")),
        None => text,
    }
}

/// A file to count and its size in bytes — the size orders the work, 0 when unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub path: String,
    pub size: u64,
}

/// Either a counted file or the reason it was left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Counted {
    File(FileCount),
    Excluded(Exclusion),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn a_verbatim_prefix_is_dropped_and_a_network_share_stays_a_share() {
        assert_eq!(plain_path(Path::new(r"\\?\C:\dev\repository")), "C:/dev/repository");
        assert_eq!(plain_path(Path::new(r"\\?\UNC\host\share\repository")), "//host/share/repository");
    }
}
