//! File rewriting for applying fixes.
//!
//! Performs position-aware replacement of broken registry paths using the byte
//! offsets captured during AST extraction. Changes are sorted by position and
//! applied in reverse order to preserve offset validity.

use crate::scanner::RegistryRef;
use anyhow::Result;
use std::path::Path;

/// A single text replacement with position information.
#[derive(Debug, Clone)]
struct Replacement {
    start: usize,
    end: usize,
    new_text: String,
}

/// Applies path replacements to a file's contents and writes the result.
///
/// For each `(old_ref, new_path)` pair, replaces the exact span of the original
/// select expression with the corrected path. Changes are applied in reverse
/// offset order to maintain position validity.
pub fn apply_changes(
    file: &Path,
    registry_name: &str,
    changes: &[(RegistryRef, String)],
) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    let new_content = apply_replacements(&content, registry_name, changes);
    std::fs::write(file, new_content)?;
    Ok(())
}

/// Applies replacements to source content, returning the modified string.
///
/// Sorts replacements by start offset (descending) and applies each in turn.
/// This ensures earlier replacements don't invalidate later offsets.
pub fn apply_replacements(
    content: &str,
    registry_name: &str,
    changes: &[(RegistryRef, String)],
) -> String {
    let mut replacements: Vec<Replacement> = changes
        .iter()
        .map(|(reference, new_path)| Replacement {
            start: reference.start_offset,
            end: reference.end_offset,
            new_text: format!("{}.{}", registry_name, new_path),
        })
        .collect();

    // Sort by start offset descending so we can apply from end to start
    replacements.sort_by(|a, b| b.start.cmp(&a.start));

    let mut result = content.to_string();
    for rep in replacements {
        if rep.start <= result.len() && rep.end <= result.len() && rep.start <= rep.end {
            result.replace_range(rep.start..rep.end, &rep.new_text);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ref(path: &str, start: usize, end: usize) -> RegistryRef {
        RegistryRef {
            path: path.to_string(),
            file: PathBuf::from("test.nix"),
            line: 1,
            column: 1,
            start_offset: start,
            end_offset: end,
        }
    }

    #[test]
    fn replaces_single_reference() {
        let content = "{ imports = [ registry.home.alice ]; }";
        //                          ^14           ^32
        let changes = vec![(make_ref("home.alice", 14, 33), "users.alice".to_string())];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(result, "{ imports = [ registry.users.alice ]; }");
    }

    #[test]
    fn replaces_multiple_references_same_line() {
        let content = "{ a = registry.foo.x; b = registry.bar.y; }";
        //                  ^6          ^20   ^26          ^40
        let changes = vec![
            (make_ref("foo.x", 6, 20), "baz.x".to_string()),
            (make_ref("bar.y", 26, 40), "qux.y".to_string()),
        ];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(result, "{ a = registry.baz.x; b = registry.qux.y; }");
    }

    #[test]
    fn preserves_surrounding_content() {
        let content = "# comment\n{ x = registry.old.path; }\n# end";
        //                              ^16             ^32
        let changes = vec![(make_ref("old.path", 16, 33), "new.path".to_string())];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(result, "# comment\n{ x = registry.new.path; }\n# end");
    }

    #[test]
    fn does_not_modify_comments_with_same_text() {
        // The key test: a comment mentions registry.old.path but we only replace
        // the actual reference at specific offsets
        let content = "# registry.old.path is deprecated\n{ x = registry.old.path; }";
        //             Comment starts at 0, actual ref at 40
        //                                                    ^40             ^56
        let changes = vec![(make_ref("old.path", 40, 57), "new.path".to_string())];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(
            result,
            "# registry.old.path is deprecated\n{ x = registry.new.path; }"
        );
    }

    #[test]
    fn handles_different_length_replacements() {
        let content = "{ x = registry.a; y = registry.b.c.d; }";
        //                  ^6         ^16   ^22            ^36
        let changes = vec![
            (make_ref("a", 6, 16), "very.long.path".to_string()),
            (make_ref("b.c.d", 22, 36), "x".to_string()),
        ];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(result, "{ x = registry.very.long.path; y = registry.x; }");
    }

    #[test]
    fn empty_changes_returns_original() {
        let content = "{ x = registry.foo; }";
        let changes: Vec<(RegistryRef, String)> = vec![];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(result, content);
    }

    #[test]
    fn handles_multiline_content() {
        let content = "{\n  imports = [\n    registry.home.alice\n  ];\n}";
        //                                  ^20               ^39
        let changes = vec![(make_ref("home.alice", 20, 39), "users.alice".to_string())];
        let result = apply_replacements(content, "registry", &changes);
        assert_eq!(
            result,
            "{\n  imports = [\n    registry.users.alice\n  ];\n}"
        );
    }
}
