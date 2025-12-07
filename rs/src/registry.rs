//! Registry evaluation and traversal.
//!
//! Shells out to `nix eval --json .#registry` to obtain the current registry
//! structure, then provides utilities for flattening it into a set of valid
//! attribute paths and printing it as a tree.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::process::Command;

/// Evaluates the flake's registry attribute by invoking `nix eval --json`.
///
/// Returns the parsed JSON value representing the registry's nested attrset structure.
/// Fails if `nix eval` returns non-zero or produces invalid JSON.
pub fn evaluate(name: &str) -> Result<serde_json::Value> {
    let output = Command::new("nix")
        .args(["eval", "--json", &format!(".#{}", name)])
        .output()
        .context("Failed to run nix eval")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nix eval failed: {}", stderr);
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse registry JSON")
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
