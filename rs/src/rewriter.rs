//! File rewriting for applying fixes.
//!
//! Performs text-based replacement of broken registry paths. This approach is
//! simple but imperfect: it does global string replacement rather than
//! position-aware AST rewriting. A future improvement would use rnix's CST
//! modification capabilities.

use crate::scanner::RegistryRef;
use anyhow::Result;
use std::path::Path;

/// Applies path replacements to a file's contents and writes the result.
///
/// For each `(old_ref, new_path)` pair, replaces occurrences of
/// `registry_name.old_ref.path` with `registry_name.new_path`. Changes are
/// written atomically by reading the entire file, transforming in memory,
/// then writing back.
pub fn apply_changes(
    file: &Path,
    registry_name: &str,
    changes: &[(RegistryRef, String)],
) -> Result<()> {
    let mut content = std::fs::read_to_string(file)?;

    for (reference, new_path) in changes {
        let old_full = format!("{}.{}", registry_name, reference.path);
        let new_full = format!("{}.{}", registry_name, new_path);
        content = content.replace(&old_full, &new_full);
    }

    std::fs::write(file, content)?;
    Ok(())
}
