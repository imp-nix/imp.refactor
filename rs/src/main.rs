//! imp-refactor: Detect and fix broken registry references in Nix projects.
//!
//! This tool scans working directory files for `registry.X.Y.Z` patterns,
//! compares them against the evaluated registry from git HEAD, and suggests
//! or applies fixes for broken references.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use rnix::SyntaxKind;
use rowan::{WalkEvent, ast::AstNode};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

/// Detect and fix broken registry references in Nix projects
#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan files and detect broken registry references
    Detect {
        /// Paths to scan (defaults to current directory)
        #[arg(short, long)]
        paths: Option<Vec<PathBuf>>,

        /// Registry attribute name in flake outputs
        #[arg(long, default_value = "registry")]
        registry_name: String,

        /// Explicit rename mappings (format: old=new)
        #[arg(long, value_parser = parse_rename)]
        rename: Vec<(String, String)>,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Show detailed diagnostics
        #[arg(short, long)]
        verbose: bool,
    },

    /// Apply suggested renames to files
    Apply {
        /// Actually modify files (default is dry-run)
        #[arg(long)]
        write: bool,

        /// Paths to scan (defaults to current directory)
        #[arg(short, long)]
        paths: Option<Vec<PathBuf>>,

        /// Registry attribute name in flake outputs
        #[arg(long, default_value = "registry")]
        registry_name: String,

        /// Explicit rename mappings (format: old=new)
        #[arg(long, value_parser = parse_rename)]
        rename: Vec<(String, String)>,
    },

    /// Show the current registry structure
    Registry {
        /// Registry attribute name in flake outputs
        #[arg(long, default_value = "registry")]
        registry_name: String,

        /// Maximum depth to display
        #[arg(long)]
        depth: Option<usize>,
    },

    /// Show what files would be scanned
    Scan {
        /// Paths to scan (defaults to current directory)
        #[arg(short, long)]
        paths: Option<Vec<PathBuf>>,
    },
}

fn parse_rename(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid rename format '{}', expected 'old=new'", s));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// A reference to a registry path found in a source file
#[derive(Debug, Clone, Serialize)]
struct RegistryRef {
    /// The dotted path after `registry.` (e.g., "home.alice")
    path: String,
    /// Source file containing the reference
    file: PathBuf,
    /// Line number (1-indexed)
    line: usize,
    /// Column number (1-indexed)
    column: usize,
}

/// Result of analyzing a broken reference
#[derive(Debug, Clone, Serialize)]
struct BrokenRef {
    #[serde(flatten)]
    reference: RegistryRef,
    /// Suggested replacement path, if found
    suggestion: Option<String>,
    /// Why no suggestion was found
    reason: Option<String>,
}

/// Diagnostics from a detection run
#[derive(Debug, Default, Serialize)]
struct Diagnostics {
    files_scanned: usize,
    total_refs: usize,
    valid_refs: usize,
    broken_refs: usize,
    suggestions_found: usize,
    unsuggestable: usize,
}

/// Detection results
#[derive(Debug, Serialize)]
struct DetectionResult {
    broken: Vec<BrokenRef>,
    diagnostics: Diagnostics,
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Detect {
            paths,
            registry_name,
            rename,
            json,
            verbose,
        } => cmd_detect(paths, &registry_name, rename, json, verbose),
        Commands::Apply {
            write,
            paths,
            registry_name,
            rename,
        } => cmd_apply(write, paths, &registry_name, rename),
        Commands::Registry {
            registry_name,
            depth,
        } => cmd_registry(&registry_name, depth),
        Commands::Scan { paths } => cmd_scan(paths),
    }
}

fn cmd_detect(
    paths: Option<Vec<PathBuf>>,
    registry_name: &str,
    rename_map: Vec<(String, String)>,
    json_output: bool,
    verbose: bool,
) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);

    // Collect nix files from working directory
    let files = collect_nix_files(&scan_paths)?;
    if verbose {
        eprintln!(
            "{} Found {} .nix files to scan",
            "info:".blue().bold(),
            files.len()
        );
    }

    // Evaluate registry from flake
    let registry = evaluate_registry(registry_name)?;
    let valid_paths = flatten_registry_paths(&registry, "");
    if verbose {
        eprintln!(
            "{} Registry contains {} valid paths",
            "info:".blue().bold(),
            valid_paths.len()
        );
    }

    // Extract references from each file
    let mut all_refs = Vec::new();
    for file in &files {
        let refs = extract_registry_refs(file, registry_name)?;
        all_refs.extend(refs);
    }

    if verbose {
        eprintln!(
            "{} Extracted {} registry references",
            "info:".blue().bold(),
            all_refs.len()
        );
    }

    // Build rename map
    let rename_map: HashMap<String, String> = rename_map.into_iter().collect();

    // Analyze references
    let mut broken = Vec::new();
    let mut valid_count = 0;

    for reference in &all_refs {
        if is_valid_path(&reference.path, &valid_paths) {
            valid_count += 1;
        } else {
            let suggestion = suggest_new_path(&reference.path, &valid_paths, &rename_map);
            let reason = if suggestion.is_none() {
                Some(determine_failure_reason(&reference.path, &valid_paths))
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

    let diagnostics = Diagnostics {
        files_scanned: files.len(),
        total_refs: all_refs.len(),
        valid_refs: valid_count,
        broken_refs: broken.len(),
        suggestions_found: broken.iter().filter(|b| b.suggestion.is_some()).count(),
        unsuggestable: broken.iter().filter(|b| b.suggestion.is_none()).count(),
    };

    let result = DetectionResult {
        broken,
        diagnostics,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_detection_result(&result, verbose);
    }

    Ok(())
}

fn cmd_apply(
    write: bool,
    paths: Option<Vec<PathBuf>>,
    registry_name: &str,
    rename_map: Vec<(String, String)>,
) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);
    let files = collect_nix_files(&scan_paths)?;
    let registry = evaluate_registry(registry_name)?;
    let valid_paths = flatten_registry_paths(&registry, "");
    let rename_map: HashMap<String, String> = rename_map.into_iter().collect();

    let mut changes_by_file: HashMap<PathBuf, Vec<(RegistryRef, String)>> = HashMap::new();

    for file in &files {
        let refs = extract_registry_refs(file, registry_name)?;
        for reference in refs {
            if !is_valid_path(&reference.path, &valid_paths)
                && let Some(new_path) = suggest_new_path(&reference.path, &valid_paths, &rename_map)
                {
                    changes_by_file
                        .entry(file.clone())
                        .or_default()
                        .push((reference, new_path));
                }
        }
    }

    if changes_by_file.is_empty() {
        println!("{} No changes to apply", "info:".blue().bold());
        return Ok(());
    }

    for (file, changes) in &changes_by_file {
        println!(
            "\n{} {}",
            if write { "Updating:" } else { "Would update:" }
                .yellow()
                .bold(),
            file.display()
        );
        for (reference, new_path) in changes {
            println!(
                "  {}:{}: {} -> {}",
                reference.line,
                reference.column,
                format!("{}.{}", registry_name, reference.path).red(),
                format!("{}.{}", registry_name, new_path).green()
            );
        }

        if write {
            apply_changes_to_file(file, registry_name, changes)?;
        }
    }

    if !write {
        println!("\n{} Use --write to apply changes", "hint:".cyan().bold());
    }

    Ok(())
}

fn cmd_registry(registry_name: &str, depth: Option<usize>) -> Result<()> {
    let registry = evaluate_registry(registry_name)?;
    print_registry_tree(&registry, "", depth.unwrap_or(usize::MAX), 0);
    Ok(())
}

fn cmd_scan(paths: Option<Vec<PathBuf>>) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);
    let files = collect_nix_files(&scan_paths)?;

    println!("Would scan {} files:", files.len());
    for file in files {
        println!("  {}", file.display());
    }

    Ok(())
}

/// Collect all .nix files from the given paths, excluding underscore-prefixed directories
fn collect_nix_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_entry(|e| !is_hidden_or_underscore(e))
        {
            let entry = entry?;
            if entry.file_type().is_file()
                && let Some(ext) = entry.path().extension()
                    && ext == "nix" {
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
        .map(|s| s.starts_with('.') || s.starts_with('_'))
        .unwrap_or(false)
}

/// Evaluate the registry by running `nix eval`
fn evaluate_registry(registry_name: &str) -> Result<serde_json::Value> {
    let output = Command::new("nix")
        .args(["eval", "--json", &format!(".#{}", registry_name)])
        .output()
        .context("Failed to run nix eval")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nix eval failed: {}", stderr);
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse registry JSON")?;

    Ok(json)
}

/// Recursively flatten registry into a set of valid dotted paths
fn flatten_registry_paths(value: &serde_json::Value, prefix: &str) -> HashSet<String> {
    let mut paths = HashSet::new();

    if let serde_json::Value::Object(map) = value {
        for (key, val) in map {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", prefix, key)
            };
            paths.insert(path.clone());
            paths.extend(flatten_registry_paths(val, &path));
        }
    }

    paths
}

/// Extract registry references from a Nix file using rnix
fn extract_registry_refs(file: &Path, registry_name: &str) -> Result<Vec<RegistryRef>> {
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("Failed to read {}", file.display()))?;

    let parse = rnix::Root::parse(&source);
    if !parse.errors().is_empty() {
        // Log parse errors but continue - file might still have extractable refs
        eprintln!(
            "{} Parse errors in {}: {:?}",
            "warn:".yellow().bold(),
            file.display(),
            parse.errors()
        );
    }

    let root = parse.tree();

    let mut refs = Vec::new();

    for event in root.syntax().preorder() {
        if let WalkEvent::Enter(node) = event
            && node.kind() == SyntaxKind::NODE_SELECT
                && let Some(path) = extract_dotted_path(&node, registry_name) {
                    let start = node.text_range().start();
                    let (line, column) = offset_to_line_col(&source, start.into());
                    refs.push(RegistryRef {
                        path,
                        file: file.to_path_buf(),
                        line,
                        column,
                    });
                }
    }

    Ok(refs)
}

/// Extract a dotted path from a select expression if it starts with registry_name
fn extract_dotted_path(
    node: &rowan::SyntaxNode<rnix::NixLanguage>,
    registry_name: &str,
) -> Option<String> {
    // Build the full path by walking up through nested selects
    let mut parts = Vec::new();
    let mut current = node.clone();

    loop {
        // Get the attribute name (rightmost part of this select)
        if let Some(attr) = current.last_child() {
            if attr.kind() == SyntaxKind::NODE_IDENT {
                parts.push(attr.text().to_string());
            } else {
                return None; // Not a simple identifier
            }
        } else {
            return None;
        }

        // Get the base expression
        if let Some(base) = current.first_child() {
            if base.kind() == SyntaxKind::NODE_SELECT {
                current = base;
            } else if base.kind() == SyntaxKind::NODE_IDENT {
                parts.push(base.text().to_string());
                break;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    parts.reverse();

    // Check if it starts with our registry name
    if parts.first().map(|s| s.as_str()) != Some(registry_name) {
        return None;
    }

    // Return everything after the registry name
    if parts.len() > 1 {
        Some(parts[1..].join("."))
    } else {
        None
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

/// Check if a path exists in the valid paths set (including partial matches)
fn is_valid_path(path: &str, valid_paths: &HashSet<String>) -> bool {
    valid_paths.contains(path)
}

/// Suggest a new path for a broken reference
fn suggest_new_path(
    old_path: &str,
    valid_paths: &HashSet<String>,
    rename_map: &HashMap<String, String>,
) -> Option<String> {
    // First, try the rename map (longest prefix wins)
    if let Some(new_path) = apply_rename_map(rename_map, old_path)
        && valid_paths.contains(&new_path) {
            return Some(new_path);
        }

    // Fall back to leaf-name heuristic
    let leaf = old_path.rsplit('.').next()?;
    let candidates: Vec<_> = valid_paths
        .iter()
        .filter(|p| p.ends_with(&format!(".{}", leaf)) || p.as_str() == leaf)
        .collect();

    if candidates.len() == 1 {
        Some(candidates[0].clone())
    } else {
        None
    }
}

fn apply_rename_map(rename_map: &HashMap<String, String>, old_path: &str) -> Option<String> {
    // Sort by length descending for longest-prefix-wins
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

fn determine_failure_reason(path: &str, valid_paths: &HashSet<String>) -> String {
    let leaf = path.rsplit('.').next().unwrap_or(path);
    let candidates: Vec<_> = valid_paths
        .iter()
        .filter(|p| p.ends_with(&format!(".{}", leaf)) || *p == leaf)
        .collect();

    if candidates.is_empty() {
        format!("No path ending in '{}' exists in registry", leaf)
    } else {
        format!(
            "Ambiguous: {} paths end in '{}': {}",
            candidates.len(),
            leaf,
            candidates
                .iter()
                .take(3)
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn print_detection_result(result: &DetectionResult, verbose: bool) {
    let d = &result.diagnostics;

    if verbose {
        println!(
            "\n{} Files: {}, Refs: {} ({} valid, {} broken)",
            "Diagnostics:".bold(),
            d.files_scanned,
            d.total_refs,
            d.valid_refs,
            d.broken_refs
        );
        println!(
            "             Suggestions: {}, Unsuggestable: {}",
            d.suggestions_found, d.unsuggestable
        );
    }

    if result.broken.is_empty() {
        println!("{} No broken references found", "ok:".green().bold());
        return;
    }

    println!(
        "\n{} {} broken reference(s):\n",
        "Found".red().bold(),
        result.broken.len()
    );

    for broken in &result.broken {
        let loc = format!(
            "{}:{}:{}",
            broken.reference.file.display(),
            broken.reference.line,
            broken.reference.column
        );

        if let Some(ref suggestion) = broken.suggestion {
            println!("  {} {}", loc.dimmed(), broken.reference.path.red());
            println!("    {} {}", "->".green(), suggestion.green());
        } else {
            println!(
                "  {} {} {}",
                loc.dimmed(),
                broken.reference.path.red(),
                format!("({})", broken.reason.as_deref().unwrap_or("no suggestion")).dimmed()
            );
        }
    }
}

fn print_registry_tree(value: &serde_json::Value, prefix: &str, max_depth: usize, depth: usize) {
    if depth >= max_depth {
        return;
    }

    if let serde_json::Value::Object(map) = value {
        let indent = "  ".repeat(depth);
        for (key, val) in map {
            let is_leaf = !val.is_object() || val.as_object().map(|m| m.is_empty()).unwrap_or(true);
            if is_leaf {
                println!("{}{}", indent, key.dimmed());
            } else {
                println!("{}{}", indent, key);
                print_registry_tree(val, prefix, max_depth, depth + 1);
            }
        }
    }
}

fn apply_changes_to_file(
    file: &Path,
    registry_name: &str,
    changes: &[(RegistryRef, String)],
) -> Result<()> {
    let source = std::fs::read_to_string(file)?;

    // Simple text replacement - for now, we do line-by-line
    // A proper implementation would use AST rewriting
    let mut result = source.clone();

    for (reference, new_path) in changes {
        let old_full = format!("{}.{}", registry_name, reference.path);
        let new_full = format!("{}.{}", registry_name, new_path);
        result = result.replace(&old_full, &new_full);
    }

    std::fs::write(file, result)?;
    Ok(())
}
