//! Files that hold no lines worth counting: binaries, lock files, minified assets.

use crate::types::Exclusion;

const LOCK_NAMES: &[&str] = &["package-lock.json", "pnpm-lock.yaml", "bun.lockb"];
/// Git's own heuristic: a NUL byte in the first 8000 bytes makes a file binary.
pub const BINARY_PROBE: usize = 8000;

/// Return why the file is left out by its name alone, or None when its content decides.
pub fn by_name(path: &str) -> Option<Exclusion> {
    let (_, name) = path.rsplit_once('/').unwrap_or(("", path));
    if name.ends_with(".lock") || LOCK_NAMES.contains(&name) {
        Some(Exclusion::Lock)
    } else if name.contains(".min.") {
        Some(Exclusion::Minified)
    } else {
        None
    }
}

/// Tell whether the first bytes of a file mark it as binary.
pub fn is_binary(probe: &[u8]) -> bool {
    probe[..probe.len().min(BINARY_PROBE)].contains(&0)
}

/// Return why the file is left out, or None when it is counted.
pub fn exclusion(path: &str, data: &[u8]) -> Option<Exclusion> {
    by_name(path).or(is_binary(data).then_some(Exclusion::Binary))
}
