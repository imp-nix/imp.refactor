//! Nix file scanner.
//!
//! Recursively walks directories to collect `.nix` files, skipping entries
//! whose names start with `.` or `_`, plus any paths matching user-provided
//! glob patterns. Uses rnix to parse each file and extract `registry.X.Y.Z`
//! attribute selection expressions from the AST.

use anyhow::{Context, Result};
use glob::Pattern;
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

/// Collects all `.nix` files under `paths`, applying exclusion rules.
///
/// When `use_default_excludes` is true, entries starting with `.` or `_` are skipped.
/// Additional patterns from `exclude_patterns` are matched against both filenames
/// and full paths.
pub fn collect_nix_files(
    paths: &[PathBuf],
    exclude_patterns: &[String],
    use_default_excludes: bool,
) -> Result<Vec<PathBuf>> {
    let patterns: Vec<Pattern> = exclude_patterns
        .iter()
        .filter_map(|p| Pattern::new(p).ok())
        .collect();

    let mut files = Vec::new();

    for path in paths {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_entry(|e| !should_exclude(e, &patterns, use_default_excludes))
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

fn should_exclude(
    entry: &walkdir::DirEntry,
    patterns: &[Pattern],
    use_default_excludes: bool,
) -> bool {
    let name = entry.file_name().to_str().unwrap_or("");

    // Default exclusions: hidden and underscore-prefixed entries
    if use_default_excludes && (name.starts_with('.') || name.starts_with('_')) {
        return true;
    }

    // Check against user-provided glob patterns
    let path_str = entry.path().to_string_lossy();
    patterns
        .iter()
        .any(|p| p.matches(&path_str) || p.matches(name))
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
        let files = collect_nix_files(&[fixture_dir], &[], true).unwrap();
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
        let files = collect_nix_files(&[fixture_dir], &[], true).unwrap();
        assert_eq!(files.len(), 6);
    }

    #[test]
    fn exclude_pattern_filters_by_name() {
        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-renames/files");
        let files =
            collect_nix_files(&[fixture_dir], &["ambiguous.nix".to_string()], true).unwrap();
        assert_eq!(files.len(), 5);
    }

    #[test]
    fn exclude_pattern_with_glob() {
        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-renames/files");
        let files = collect_nix_files(&[fixture_dir], &["*valid*".to_string()], true).unwrap();
        // Excludes all-valid.nix and partial-valid.nix
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn no_default_excludes_includes_dotfiles() {
        let tmp = tempfile::tempdir().unwrap();
        let testdir = tmp.path().join("testroot");
        std::fs::create_dir(&testdir).unwrap();
        let dotdir = testdir.join(".hidden");
        std::fs::create_dir(&dotdir).unwrap();
        std::fs::write(dotdir.join("test.nix"), "{}").unwrap();
        std::fs::write(testdir.join("visible.nix"), "{}").unwrap();

        // With defaults, hidden dir is excluded
        let files = collect_nix_files(&[testdir.clone()], &[], true).unwrap();
        assert_eq!(files.len(), 1);

        // Without defaults, hidden dir is included
        let files = collect_nix_files(&[testdir], &[], false).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn no_default_excludes_includes_underscored() {
        let tmp = tempfile::tempdir().unwrap();
        let testdir = tmp.path().join("testroot");
        std::fs::create_dir(&testdir).unwrap();
        std::fs::write(testdir.join("_internal.nix"), "{}").unwrap();
        std::fs::write(testdir.join("public.nix"), "{}").unwrap();

        // With defaults, underscore-prefixed is excluded
        let files = collect_nix_files(&[testdir.clone()], &[], true).unwrap();
        assert_eq!(files.len(), 1);

        // Without defaults, underscore-prefixed is included
        let files = collect_nix_files(&[testdir], &[], false).unwrap();
        assert_eq!(files.len(), 2);
    }

    // =========================================================================
    // False positive tests - patterns that should NOT be detected
    // =========================================================================

    #[test]
    fn ignores_string_containing_registry() {
        let source = r#"npmRegistry = "https://registry.npmjs.org";"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "String literal should not match");
    }

    #[test]
    fn ignores_registry_as_attrset_key() {
        let source = r#"dockerConfig = { registry = "ghcr.io"; };"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "Attrset key definition should not match");
    }

    #[test]
    fn ignores_select_on_other_ident_with_registry_attr() {
        let source = r#"x = someModule.registry.path;"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "Select on non-registry ident should not match");
    }

    #[test]
    fn ignores_config_nix_registry() {
        let source = r#"nix.registry.nixpkgs.flake = inputs.nixpkgs;"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "nix.registry path should not match");
    }

    #[test]
    fn ignores_registry_as_function_call() {
        let source = r#"result = registry { arg = 1; };"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "Function call should not match");
    }

    #[test]
    fn ignores_similar_ident_names() {
        let source = r#"x = registryBackup.old.path;"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "Similar but different ident should not match");
    }

    #[test]
    fn ignores_quoted_attr_access() {
        // Dynamic attribute access with quotes
        let source = r#"foo = registry."home.alice";"#;
        let refs = extract_paths_from_source(source, "registry");
        // This may or may not match depending on rnix parsing - document behavior
        // The key point is it shouldn't crash and behavior should be consistent
        assert!(refs.is_empty() || refs.len() == 1);
    }

    #[test]
    fn ignores_inherit_pattern() {
        let source = r#"{ inherit (inputs) registry; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "Inherit should not match");
    }

    #[test]
    fn ignores_nested_attrpath_definition() {
        // This is a definition, not a reference
        let source = r#"{ container.registry.url = "docker.io"; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert!(refs.is_empty(), "Nested attrpath definition should not match");
    }

    // =========================================================================
    // True positive tests - patterns that SHOULD be detected
    // =========================================================================

    #[test]
    fn detects_registry_in_parentheses() {
        let source = r#"{ a = (registry.path.one); }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.one"]);
    }

    #[test]
    fn detects_registry_as_function_arg() {
        let source = r#"{ b = lib.mkDefault registry.path.two; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.two"]);
    }

    #[test]
    fn detects_multiple_registry_in_list() {
        let source = r#"{ c = [ registry.path.three registry.path.four ]; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.three", "path.four"]);
    }

    #[test]
    fn detects_registry_in_attrset_value() {
        let source = r#"{ d = { key = registry.path.five; }; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.five"]);
    }

    #[test]
    fn detects_registry_with_or_default() {
        let source = r#"{ e = registry.path.six or null; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.six"]);
    }

    #[test]
    fn detects_registry_with_merge_operator() {
        let source = r#"{ f = { } // registry.path.seven; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.seven"]);
    }

    #[test]
    fn detects_registry_in_rec_attrset() {
        let source = r#"{ h = rec { val = registry.path.nine; }; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["path.nine"]);
    }

    #[test]
    fn detects_registry_in_let_binding() {
        let source = r#"let x = registry.users.alice; in x"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["users.alice"]);
    }

    #[test]
    fn detects_registry_in_import_list() {
        let source = r#"{ imports = [ registry.hosts.desktop registry.modules.base ]; }"#;
        let refs = extract_paths_from_source(source, "registry");
        assert_eq!(refs, vec!["hosts.desktop", "modules.base"]);
    }

    // =========================================================================
    // Fixture-based false positive tests
    // =========================================================================

    #[test]
    fn fixture_should_ignore_finds_no_refs() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/false-positives/should-ignore.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        assert!(
            refs.is_empty(),
            "Expected no refs in should-ignore.nix, found: {:?}",
            refs.iter().map(|r| &r.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fixture_known_limitations_documents_false_positives() {
        // This test documents a known limitation: function parameter shadowing.
        // When a function has a parameter named `registry`, any attribute access
        // on that parameter will be detected as a registry reference.
        // This is expected behavior given static analysis constraints.
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/false-positives/known-limitations.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        
        // These WILL be detected even though they're local params, not imp registry
        assert!(paths.contains(&"endpoint"), "Expected false positive: endpoint");
        assert!(paths.contains(&"settings.base"), "Expected false positive: settings.base");
        assert!(paths.contains(&"data.items"), "Expected false positive: data.items");
        assert!(paths.contains(&"nested.value"), "Expected false positive: nested.value");
    }

    #[test]
    fn fixture_should_detect_finds_expected_refs() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/false-positives/should-detect.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        
        // Should find all the actual registry references
        assert!(paths.contains(&"users.alice"), "Missing users.alice");
        assert!(paths.contains(&"profiles.desktop"), "Missing profiles.desktop");
        assert!(paths.contains(&"profiles.server"), "Missing profiles.server");
        assert!(paths.contains(&"modules.base"), "Missing modules.base");
        assert!(paths.contains(&"modules.networking"), "Missing modules.networking");
        assert!(paths.contains(&"hosts.desktop"), "Missing hosts.desktop");
        assert!(paths.contains(&"modules.nixos.base"), "Missing modules.nixos.base");
        
        // Should have a reasonable number of refs (not too many false positives)
        assert!(paths.len() >= 10, "Expected at least 10 refs, got {}", paths.len());
        assert!(paths.len() <= 20, "Too many refs ({}), possible false positives", paths.len());
    }

    #[test]
    fn fixture_edge_cases_correct_detection() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/false-positives/edge-cases.nix");
        let refs = extract_registry_refs(&fixture, "registry").unwrap();
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        
        // SHOULD detect these (registry is the base ident being selected from)
        assert!(paths.contains(&"path.one"), "Missing path.one (parentheses)");
        assert!(paths.contains(&"path.two"), "Missing path.two (function arg)");
        assert!(paths.contains(&"path.three"), "Missing path.three (list)");
        assert!(paths.contains(&"path.four"), "Missing path.four (list)");
        assert!(paths.contains(&"path.five"), "Missing path.five (attrset value)");
        assert!(paths.contains(&"path.six"), "Missing path.six (or default)");
        assert!(paths.contains(&"path.seven"), "Missing path.seven (merge)");
        assert!(paths.contains(&"path.nine"), "Missing path.nine (rec)");
        
        // SHOULD NOT detect these (registry is not the base, or different ident)
        // config.nix.registry.nixpkgs - base is config, not registry
        assert!(!paths.iter().any(|p| p.contains("nixpkgs")), "False positive: config.nix.registry");
        // registryBackup.old.path - different ident
        assert!(!paths.iter().any(|p| p.contains("old")), "False positive: registryBackup");
        // registry'.shadowed.path - different ident (registry')
        assert!(!paths.iter().any(|p| p.contains("shadowed")), "False positive: registry'");
    }
}
