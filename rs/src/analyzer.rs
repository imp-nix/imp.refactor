//! Reference analysis and suggestion generation.
//!
//! Compares extracted registry references against the set of valid paths,
//! identifies broken references, and attempts to suggest corrections using
//! explicit rename mappings or a leaf-name heuristic.

use crate::scanner::RegistryRef;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A broken registry reference with optional fix suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct BrokenRef {
    #[serde(flatten)]
    pub reference: RegistryRef,
    /// Suggested replacement path if one could be determined.
    pub suggestion: Option<String>,
    /// Explanation when no suggestion exists.
    pub reason: Option<String>,
}

/// Summary statistics from a detection run.
#[derive(Debug, Default, Serialize)]
pub struct Diagnostics {
    pub files_scanned: usize,
    pub total_refs: usize,
    pub valid_refs: usize,
    pub broken_refs: usize,
    pub suggestions_found: usize,
    pub unsuggestable: usize,
}

/// Complete detection results.
#[derive(Debug, Serialize)]
pub struct DetectionResult {
    pub broken: Vec<BrokenRef>,
    pub diagnostics: Diagnostics,
}

/// Analyzes references against valid registry paths.
///
/// Returns broken references with suggestions where possible. For each reference:
/// 1. Checks if path exists in `valid_paths`
/// 2. For broken refs, tries `rename_map` (longest prefix wins)
/// 3. Falls back to leaf-name matching if rename map fails
pub fn analyze(
    refs: &[RegistryRef],
    valid_paths: &HashSet<String>,
    rename_map: &HashMap<String, String>,
) -> (Vec<BrokenRef>, usize) {
    let mut broken = Vec::new();
    let mut valid_count = 0;

    for reference in refs {
        if valid_paths.contains(&reference.path) {
            valid_count += 1;
        } else {
            let suggestion = suggest_path(&reference.path, valid_paths, rename_map);
            let reason = if suggestion.is_none() {
                Some(failure_reason(&reference.path, valid_paths))
            } else {
                None
            };
            broken.push(BrokenRef {
                reference: reference.clone(),
                suggestion,
                reason,
            });
        }
    }

    (broken, valid_count)
}

/// Attempts to find a valid replacement for `old_path`.
///
/// First applies explicit renames from `rename_map`, selecting the longest matching
/// prefix. If the renamed path exists in `valid_paths`, returns it. Otherwise falls
/// back to searching for paths ending with the same leaf attribute name; returns
/// that path only if exactly one candidate exists.
pub fn suggest_path(
    old_path: &str,
    valid_paths: &HashSet<String>,
    rename_map: &HashMap<String, String>,
) -> Option<String> {
    if let Some(new_path) = apply_rename_map(rename_map, old_path) {
        if valid_paths.contains(&new_path) {
            return Some(new_path);
        }
    }

    suggest_by_leaf(old_path, valid_paths)
}

/// Applies rename mappings using longest-prefix-wins semantics.
///
/// If `old_path` starts with a key from `rename_map`, substitutes that prefix
/// with the corresponding value. Exact matches take precedence over prefix matches.
fn apply_rename_map(rename_map: &HashMap<String, String>, old_path: &str) -> Option<String> {
    let mut prefixes: Vec<_> = rename_map.keys().collect();
    prefixes.sort_by_key(|k| std::cmp::Reverse(k.len()));

    for prefix in prefixes {
        if old_path == prefix {
            return Some(rename_map[prefix].clone());
        }
        if old_path.starts_with(&format!("{}.", prefix)) {
            let suffix = &old_path[prefix.len() + 1..];
            return Some(format!("{}.{}", rename_map[prefix], suffix));
        }
    }

    None
}

/// Searches for valid paths sharing the same leaf attribute name.
///
/// Given `old_path = "foo.bar.baz"`, looks for paths in `valid_paths` that end
/// with `.baz` or equal `baz`. Returns the path only if exactly one match exists;
/// ambiguous matches return `None`.
fn suggest_by_leaf(old_path: &str, valid_paths: &HashSet<String>) -> Option<String> {
    let leaf = old_path.rsplit('.').next()?;
    let suffix = format!(".{}", leaf);

    let candidates: Vec<_> = valid_paths
        .iter()
        .filter(|p| p.ends_with(&suffix) || p.as_str() == leaf)
        .collect();

    if candidates.len() == 1 {
        Some(candidates[0].clone())
    } else {
        None
    }
}

/// Explains why no suggestion could be generated.
fn failure_reason(path: &str, valid_paths: &HashSet<String>) -> String {
    let leaf = path.rsplit('.').next().unwrap_or(path);
    let suffix = format!(".{}", leaf);

    let candidates: Vec<_> = valid_paths
        .iter()
        .filter(|p| p.ends_with(&suffix) || *p == leaf)
        .collect();

    if candidates.is_empty() {
        format!("No path ending in '{}' exists in registry", leaf)
    } else {
        let shown: Vec<_> = candidates.iter().take(3).map(|s| s.as_str()).collect();
        format!(
            "Ambiguous: {} paths end in '{}': {}",
            candidates.len(),
            leaf,
            shown.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn renames(items: &[(&str, &str)]) -> HashMap<String, String> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn suggest_by_leaf_unique_match() {
        let valid = paths(&["users.alice", "users.bob"]);
        assert_eq!(
            suggest_by_leaf("home.alice", &valid),
            Some("users.alice".to_string())
        );
    }

    #[test]
    fn suggest_by_leaf_ambiguous_returns_none() {
        let valid = paths(&["users.alice", "admins.alice"]);
        assert_eq!(suggest_by_leaf("home.alice", &valid), None);
    }

    #[test]
    fn suggest_by_leaf_no_match_returns_none() {
        let valid = paths(&["users.bob", "users.carol"]);
        assert_eq!(suggest_by_leaf("home.alice", &valid), None);
    }

    #[test]
    fn apply_rename_map_exact_match() {
        let map = renames(&[("home", "users")]);
        assert_eq!(apply_rename_map(&map, "home"), Some("users".to_string()));
    }

    #[test]
    fn apply_rename_map_prefix_match() {
        let map = renames(&[("home", "users")]);
        assert_eq!(
            apply_rename_map(&map, "home.alice"),
            Some("users.alice".to_string())
        );
    }

    #[test]
    fn apply_rename_map_no_match() {
        let map = renames(&[("home", "users")]);
        assert_eq!(apply_rename_map(&map, "other.path"), None);
    }

    #[test]
    fn apply_rename_map_longest_prefix_wins() {
        let map = renames(&[("home", "users"), ("home.alice", "admins.alice")]);
        assert_eq!(
            apply_rename_map(&map, "home.alice.settings"),
            Some("admins.alice.settings".to_string())
        );
    }

    #[test]
    fn suggest_path_uses_rename_map_first() {
        let valid = paths(&["users.alice", "users.bob"]);
        let map = renames(&[("home", "users")]);
        assert_eq!(
            suggest_path("home.alice", &valid, &map),
            Some("users.alice".to_string())
        );
    }

    #[test]
    fn suggest_path_falls_back_to_leaf() {
        let valid = paths(&["users.alice", "users.bob"]);
        let map = renames(&[]);
        assert_eq!(
            suggest_path("home.alice", &valid, &map),
            Some("users.alice".to_string())
        );
    }

    #[test]
    fn suggest_path_rename_must_exist_in_valid() {
        let valid = paths(&["other.charlie"]);
        let map = renames(&[("home", "users")]);
        assert_eq!(suggest_path("home.alice", &valid, &map), None);
    }

    #[test]
    fn suggest_deep_nested_paths() {
        let valid = paths(&[
            "users.alice.programs.editor",
            "users.alice.programs.zsh",
            "services.database.postgresql",
        ]);
        let map = renames(&[]);
        assert_eq!(
            suggest_path("home.alice.programs.editor", &valid, &map),
            Some("users.alice.programs.editor".to_string())
        );
    }

    #[test]
    fn suggest_with_multiple_same_depth_ambiguity() {
        let valid = paths(&["services.database.postgresql", "legacy.database.postgresql"]);
        let map = renames(&[]);
        assert_eq!(suggest_path("old.db.postgresql", &valid, &map), None);
    }
}
