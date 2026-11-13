//! The top level of the folded view: every kind and every role in one of a few groups.

use crate::types::{Kind, Role};

/// The kind groups in the order they are shown.
pub const KINDS: [&str; 5] = ["code", "docs", "data", "blank", "other"];
/// The role groups in the order they are shown.
pub const ROLES: [&str; 4] = ["engineering", "documentation", "data", "other"];

/// The group a kind folds into: docstrings, comments and prose are all documentation.
pub fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Code => "code",
        Kind::Docstring | Kind::Comment | Kind::Prose => "docs",
        Kind::Data => "data",
        Kind::Blank => "blank",
        Kind::Unclassified => "other",
    }
}

/// The group a role folds into: everything that builds, checks or configures the product is engineering.
pub fn role(role: Role) -> &'static str {
    match role {
        Role::Source | Role::Test | Role::Configuration | Role::Template | Role::Script => "engineering",
        Role::Documentation => "documentation",
        Role::Data => "data",
        Role::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_and_role_lands_in_a_shown_group() {
        assert!(Kind::ALL.into_iter().all(|each| KINDS.contains(&kind(each))));
        assert!(Role::ALL.into_iter().all(|each| ROLES.contains(&role(each))));
        assert_eq!(kind(Kind::Prose), "docs");
        assert_eq!(role(Role::Test), "engineering");
    }
}
