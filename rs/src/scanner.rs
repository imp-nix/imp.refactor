//! Nix file scanner.
//!
//! Recursively walks directories to collect `.nix` files, skipping entries
//! whose names start with `.` or `_`. Uses rnix to parse each file and extract
//! `registry.X.Y.Z` attribute selection expressions from the AST.

use anyhow::{Context, Result};
use rnix::SyntaxKind;
use rowan::{WalkEvent, ast::AstNode};
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A reference to a registry path found in source.
#[derive(Debug, Clone, Serialize)]
pub struct RegistryRef {
    /// Dotted path after `registry.`, e.g. `"home.alice"`.
    pub path: String,
    /// Source file containing the reference.
    pub file: PathBuf,
    /// Line number, 1-indexed.
    pub line: usize,
    /// Column number, 1-indexed.
    pub column: usize,
    /// Byte offset of the start of the entire select expression.
    pub start_offset: usize,
    /// Byte offset of the end of the entire select expression.
    pub end_offset: usize,
}

/// Collects all `.nix` files under `paths`, excluding hidden and underscore-prefixed directories.
pub fn collect_nix_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_entry(|e| !is_hidden_or_underscore(e))
        {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "nix")
            {
                files.push(entry.into_path());
            }
        }
    }

    Ok(files)
}

fn is_hidden_or_underscore(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.') || s.starts_with('_'))
}

/// Parses a Nix file and extracts all `registry_name.X.Y...` attribute selections.
///
/// Walks the rnix AST looking for `NODE_SELECT` nodes. For each, reconstructs
/// the full dotted path by traversing nested selects, then checks whether the
/// root identifier matches `registry_name`.
pub fn extract_registry_refs(file: &Path, registry_name: &str) -> Result<Vec<RegistryRef>> {
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("Failed to read {}", file.display()))?;

    let parse = rnix::Root::parse(&source);
    if !parse.errors().is_empty() {
        eprintln!(
            "warn: Parse errors in {}: {:?}",
            file.display(),
            parse.errors()
        );
    }

    let root = parse.tree();
    let mut refs = Vec::new();

    for event in root.syntax().preorder() {
        if let WalkEvent::Enter(node) = event {
            if node.kind() == SyntaxKind::NODE_SELECT {
                if let Some(path) = extract_dotted_path(&node, registry_name) {
                    let range = node.text_range();
                    let start: usize = range.start().into();
                    let end: usize = range.end().into();
                    let (line, column) = offset_to_line_col(&source, start);
                    refs.push(RegistryRef {
                        path,
                        file: file.to_path_buf(),
                        line,
                        column,
                        start_offset: start,
                        end_offset: end,
                    });
                }
            }
        }
    }

    Ok(refs)
}

/// Reconstructs a dotted attribute path from a `NODE_SELECT` node.
///
/// rnix parses `registry.home.alice` as:
/// ```text
/// NODE_SELECT
///   NODE_IDENT (registry)
///   NODE_ATTRPATH
///     NODE_IDENT (home)
///     NODE_IDENT (alice)
/// ```
///
/// Returns `Some(path)` if the base identifier equals `registry_name`,
/// where `path` is the attrpath joined by `.`.
fn extract_dotted_path(
    node: &rowan::SyntaxNode<rnix::NixLanguage>,
    registry_name: &str,
) -> Option<String> {
    let mut children = node.children();

    let base = children.next()?;
    if base.kind() != SyntaxKind::NODE_IDENT {
        return None;
    }
    if base.text().to_string() != registry_name {
        return None;
    }

    let attrpath = children.next()?;
    if attrpath.kind() != SyntaxKind::NODE_ATTRPATH {
        return None;
    }

    let parts: Vec<String> = attrpath
        .children()
        .filter(|c| c.kind() == SyntaxKind::NODE_IDENT)
        .map(|c| c.text().to_string())
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Extracts registry paths from Nix source code.
///
/// Parses the source string and returns all dotted paths following `registry_name.`.
/// Unlike `extract_registry_refs`, this operates on strings directly for testing.
#[cfg(test)]
pub fn extract_paths_from_source(source: &str, registry_name: &str) -> Vec<String> {
    let parse = rnix::Root::parse(source);
    let root = parse.tree();
    let mut paths = Vec::new();

    for event in root.syntax().preorder() {
        if let WalkEvent::Enter(node) = event {
            if node.kind() == SyntaxKind::NODE_SELECT {
                if let Some(path) = extract_dotted_path(&node, registry_name) {
                    paths.push(path);
                }
            }
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_registry_reference() {
        let source = r#"
            { registry, ... }:
            { imports = [ registry.home.alice ]; }
        "#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["home.alice"]);
    }

    #[test]
    fn extracts_multiple_registry_references() {
        let source = r#"
            { registry, ... }:
            {
                imports = [
                    registry.home.alice
                    registry.modules.nixos
                    registry.hosts.server
                ];
            }
        "#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["home.alice", "modules.nixos", "hosts.server"]);
    }

    #[test]
    fn extracts_references_from_same_line() {
        let source = "{ foo = registry.a.b; bar = registry.c.d; }";
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["a.b", "c.d"]);
    }

    #[test]
    fn ignores_non_registry_patterns() {
        let source = r#"
            { pkgs, lib, ... }:
            {
                foo = "registry";
                bar = pkgs.hello;
            }
        "#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty());
    }

    #[test]
    fn handles_deep_nesting() {
        let source = "registry.a.b.c.d.e";
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["a.b.c.d.e"]);
    }

    #[test]
    fn extracts_with_custom_registry_name() {
        let source = r#"
            { impRegistry, ... }:
            { imports = [ impRegistry.home.alice ]; }
        "#;
        let refs = extract_paths_from_source(source, "impRegistry");
        assert_eq!(refs, vec!["home.alice"]);
    }

    #[test]
    fn ignores_registry_without_path() {
        let source = "x = registry;";
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty());
    }

    #[test]
    fn extracts_from_fixture_file() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/migrate-test/outputs/config-a.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["home.alice", "modules.nixos"]);
    }

    #[test]
    fn extracts_from_multi_rename_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/complex-renames/files/multi-rename.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "home.alice.programs.editor",
                "home.bob.shell",
                "svc.database.postgresql",
                "svc.web.nginx",
                "mods.profiles.desktop.gnome",
            ]
        );
    }

    #[test]
    fn collects_nix_files_from_fixture() {
        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/migrate-test/outputs");
        let files = collect_nix_files(&[fixture_dir]).unwrap();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn extracts_from_config_b_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/migrate-test/outputs/config-b.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["users.alice", "mods.nixos"]);
    }

    #[test]
    fn extracts_from_mixed_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/migrate-test/outputs/mixed.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["hosts.server", "users.bob"]);
    }

    #[test]
    fn extracts_from_deep_nesting_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/complex-renames/files/deep-nesting.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "svc.db.postgresql",
                "svc.db.redis",
                "svc.http.nginx",
                "svc.http.caddy",
                "utils.helpers.strings",
            ]
        );
    }

    #[test]
    fn extracts_from_ambiguous_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/complex-renames/files/ambiguous.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "old.programs.editor",
                "old.desktop.gnome",
                "config.server.minimal",
                "configs.base",
            ]
        );
    }

    #[test]
    fn extracts_from_partial_valid_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/complex-renames/files/partial-valid.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "users.alice.programs.editor",
                "services.database.postgresql",
                "profiles.desktop.gnome",
                "home.bob.shell",
                "svc.web.caddy",
            ]
        );
    }

    #[test]
    fn extracts_from_all_valid_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/complex-renames/files/all-valid.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "users.alice.programs.editor",
                "users.alice.programs.zsh",
                "users.bob.shell",
                "services.database.postgresql",
                "services.web.nginx",
                "profiles.desktop.gnome",
                "lib.helpers.strings",
            ]
        );
    }

    #[test]
    fn collects_nix_files_from_complex_renames() {
        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-renames/files");
        let files = collect_nix_files(&[fixture_dir]).unwrap();
        assert_eq!(files.len(), 6);
    }
}
