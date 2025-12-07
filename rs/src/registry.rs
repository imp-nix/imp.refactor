//! Registry evaluation and traversal.
//!
//! Shells out to `nix eval --json .#registry` to obtain the current registry
//! structure, then provides utilities for flattening it into a set of valid
//! attribute paths and printing it as a tree.
//!
//! Supports evaluating against a specific git ref (e.g., HEAD, HEAD^, main)
//! to compare working tree changes against the committed registry state.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::process::Command;

/// Evaluates the flake's registry attribute by invoking `nix eval --json`.
///
/// If `git_ref` is provided, evaluates the registry from that git ref using
/// `builtins.getFlake`. Otherwise evaluates from the current working tree.
///
/// Returns the parsed JSON value representing the registry's nested attrset structure.
/// Fails if `nix eval` returns non-zero or produces invalid JSON.
pub fn evaluate(name: &str, git_ref: Option<&str>) -> Result<serde_json::Value> {
    let output = match git_ref {
        Some(r) => {
            // Resolve git ref to commit hash for use in flake URL
            let commit = resolve_git_ref(r)?;
            let expr = format!("(builtins.getFlake \"git+file:.?rev={}\").{}", commit, name);
            Command::new("nix")
                .args(["eval", "--json", "--expr", &expr])
                .output()
                .context("Failed to run nix eval")?
        }
        None => {
            let attr = format!(".#{}", name);
            Command::new("nix")
                .args(["eval", "--json", &attr])
                .output()
                .context("Failed to run nix eval")?
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nix eval failed: {}", stderr);
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse registry JSON")
}

/// Resolves a git ref (branch, tag, HEAD, HEAD^, etc.) to a full commit hash.
fn resolve_git_ref(git_ref: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", git_ref])
        .output()
        .context("Failed to run git rev-parse")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git rev-parse failed for '{}': {}", git_ref, stderr.trim());
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(commit)
}

/// Recursively flattens a registry JSON value into all valid dotted paths.
///
/// Given `{ home = { alice = {}; bob = {}; }; }`, returns the set
/// `["home", "home.alice", "home.bob"]`.
pub fn flatten_paths(value: &serde_json::Value, prefix: &str) -> HashSet<String> {
    let mut paths = HashSet::new();

    if let serde_json::Value::Object(map) = value {
        for (key, val) in map {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", prefix, key)
            };
            paths.insert(path.clone());
            paths.extend(flatten_paths(val, &path));
        }
    }

    paths
}

/// Prints the registry as an indented tree to stdout.
///
/// Leaf nodes (non-objects or empty objects) are dimmed. Recurses up to
/// `max_depth` levels.
pub fn print_tree(value: &serde_json::Value, max_depth: usize, depth: usize) {
    if depth >= max_depth {
        return;
    }

    if let serde_json::Value::Object(map) = value {
        let indent = "  ".repeat(depth);
        for (key, val) in map {
            let is_leaf = !val.is_object() || val.as_object().is_some_and(|m| m.is_empty());
            if is_leaf {
                println!("{}{}", indent, key.dimmed());
            } else {
                println!("{}{}", indent, key);
                print_tree(val, max_depth, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_paths_empty() {
        let value = json!({});
        let paths = flatten_paths(&value, "");
        assert!(paths.is_empty());
    }

    #[test]
    fn flatten_paths_single_level() {
        let value = json!({ "foo": {}, "bar": {} });
        let paths = flatten_paths(&value, "");
        assert!(paths.contains("foo"));
        assert!(paths.contains("bar"));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn flatten_paths_nested() {
        let value = json!({
            "home": {
                "alice": {},
                "bob": {}
            },
            "modules": {}
        });
        let paths = flatten_paths(&value, "");
        assert!(paths.contains("home"));
        assert!(paths.contains("home.alice"));
        assert!(paths.contains("home.bob"));
        assert!(paths.contains("modules"));
        assert_eq!(paths.len(), 4);
    }

    #[test]
    fn flatten_paths_with_prefix() {
        let value = json!({ "alice": {}, "bob": {} });
        let paths = flatten_paths(&value, "users");
        assert!(paths.contains("users.alice"));
        assert!(paths.contains("users.bob"));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    #[ignore] // Requires git repository context (not available in Nix sandbox)
    fn resolve_git_ref_head() {
        // This test requires running in a git repository
        let result = resolve_git_ref("HEAD");
        assert!(result.is_ok());
        let commit = result.unwrap();
        // Git commit hashes are 40 hex characters
        assert_eq!(commit.len(), 40);
        assert!(commit.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    #[ignore] // Requires git repository context (not available in Nix sandbox)
    fn resolve_git_ref_invalid() {
        let result = resolve_git_ref("nonexistent-ref-that-does-not-exist");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("git rev-parse failed"));
    }
}
