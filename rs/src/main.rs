//! CLI entrypoint for imp-refactor.
//!
//! This binary wraps the imp_refactor library to provide a command-line
//! interface for detecting and fixing broken registry references.

mod cli;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use dialoguer::Select;
use std::collections::HashMap;
use std::path::PathBuf;

use cli::{Args, Commands};
use imp_refactor::{
    BrokenRef, DetectionResult, Diagnostics, RegistryRef, analyzer, registry, rewriter, scanner,
};

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Detect {
            paths,
            exclude,
            no_default_excludes,
            registry_name,
            git_ref,
            rename,
            json,
            verbose,
        } => cmd_detect(
            paths,
            &exclude,
            !no_default_excludes,
            &registry_name,
            git_ref.as_deref(),
            rename,
            json,
            verbose,
        ),

        Commands::Apply {
            write,
            interactive,
            paths,
            exclude,
            no_default_excludes,
            registry_name,
            git_ref,
            rename,
        } => cmd_apply(
            write,
            interactive,
            paths,
            &exclude,
            !no_default_excludes,
            &registry_name,
            git_ref.as_deref(),
            rename,
        ),

        Commands::Registry {
            registry_name,
            git_ref,
            depth,
        } => cmd_registry(&registry_name, git_ref.as_deref(), depth),

        Commands::Scan {
            paths,
            exclude,
            no_default_excludes,
        } => cmd_scan(paths, &exclude, !no_default_excludes),
    }
}

fn cmd_detect(
    paths: Option<Vec<PathBuf>>,
    exclude: &[String],
    use_default_excludes: bool,
    registry_name: &str,
    git_ref: Option<&str>,
    rename_map: Vec<(String, String)>,
    json_output: bool,
    verbose: bool,
) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);

    let files = scanner::collect_nix_files(&scan_paths, exclude, use_default_excludes)?;
    if verbose {
        eprintln!(
            "{} Found {} .nix files to scan",
            "info:".blue().bold(),
            files.len()
        );
        if let Some(ref r) = git_ref {
            eprintln!(
                "{} Evaluating registry from git ref '{}'",
                "info:".blue().bold(),
                r
            );
        }
    }

    let reg = registry::evaluate(registry_name, git_ref)?;
    let valid_paths = registry::flatten_paths(&reg, "");
    if verbose {
        eprintln!(
            "{} Registry contains {} valid paths",
            "info:".blue().bold(),
            valid_paths.len()
        );
    }

    let mut all_refs = Vec::new();
    for file in &files {
        let refs = scanner::extract_registry_refs(file, registry_name)?;
        all_refs.extend(refs);
    }

    if verbose {
        eprintln!(
            "{} Extracted {} registry references",
            "info:".blue().bold(),
            all_refs.len()
        );
    }

    let rename_map: HashMap<String, String> = rename_map.into_iter().collect();
    let (broken, valid_count) = analyzer::analyze(&all_refs, &valid_paths, &rename_map);

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
    interactive: bool,
    paths: Option<Vec<PathBuf>>,
    exclude: &[String],
    use_default_excludes: bool,
    registry_name: &str,
    git_ref: Option<&str>,
    rename_map: Vec<(String, String)>,
) -> Result<()> {
    // Interactive implies write
    let should_write = write || interactive;

    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);
    let files = scanner::collect_nix_files(&scan_paths, exclude, use_default_excludes)?;
    let reg = registry::evaluate(registry_name, git_ref)?;
    let valid_paths = registry::flatten_paths(&reg, "");
    let rename_map: HashMap<String, String> = rename_map.into_iter().collect();

    let mut changes_by_file: HashMap<PathBuf, Vec<(RegistryRef, String)>> = HashMap::new();

    for file in &files {
        let refs = scanner::extract_registry_refs(file, registry_name)?;
        for reference in refs {
            if !valid_paths.contains(&reference.path) {
                if let Some(new_path) =
                    analyzer::suggest_path(&reference.path, &valid_paths, &rename_map)
                {
                    changes_by_file
                        .entry(file.clone())
                        .or_default()
                        .push((reference, new_path));
                }
            }
        }
    }

    if changes_by_file.is_empty() {
        println!("{} No changes to apply", "info:".blue().bold());
        return Ok(());
    }

    // Sort files for consistent output
    let mut files_with_changes: Vec<_> = changes_by_file.into_iter().collect();
    files_with_changes.sort_by(|a, b| a.0.cmp(&b.0));

    let total_files = files_with_changes.len();
    let total_changes: usize = files_with_changes.iter().map(|(_, c)| c.len()).sum();

    if interactive {
        println!(
            "\n{} {} change(s) in {} file(s)\n",
            "Found".cyan().bold(),
            total_changes,
            total_files
        );
    }

    let mut applied_files = 0;
    let mut applied_changes = 0;
    let mut skipped_files = 0;

    for (file, changes) in &files_with_changes {
        // Print file header and changes
        let action = if should_write && !interactive {
            "Updating:"
        } else if interactive {
            "File:"
        } else {
            "Would update:"
        };
        println!("{} {}", action.yellow().bold(), file.display());

        for (reference, new_path) in changes {
            println!(
                "  {}:{}: {} -> {}",
                reference.line,
                reference.column,
                format!("{}.{}", registry_name, reference.path).red(),
                format!("{}.{}", registry_name, new_path).green()
            );
        }

        if interactive {
            let choice = prompt_file_action(changes.len())?;
            match choice {
                FileAction::Apply => {
                    rewriter::apply_changes(file, registry_name, changes)?;
                    println!(
                        "  {} Applied {} change(s)\n",
                        "ok:".green().bold(),
                        changes.len()
                    );
                    applied_files += 1;
                    applied_changes += changes.len();
                }
                FileAction::Skip => {
                    println!("  {} Skipped\n", "info:".blue().bold());
                    skipped_files += 1;
                }
                FileAction::Abort => {
                    println!("\n{} Aborted", "info:".blue().bold());
                    return Ok(());
                }
            }
        } else if should_write {
            rewriter::apply_changes(file, registry_name, changes)?;
            applied_files += 1;
            applied_changes += changes.len();
        }

        // Add blank line between files in non-interactive mode
        if !interactive {
            println!();
        }
    }

    // Summary
    if interactive {
        println!(
            "{} Applied {} change(s) in {} file(s), skipped {} file(s)",
            "Done:".green().bold(),
            applied_changes,
            applied_files,
            skipped_files
        );
    } else if !should_write {
        println!("{} Use --write to apply changes", "hint:".cyan().bold());
    }

    Ok(())
}

/// User's choice for handling a file's changes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FileAction {
    Apply,
    Skip,
    Abort,
}

/// Prompts the user to decide what to do with changes for a file.
fn prompt_file_action(change_count: usize) -> Result<FileAction> {
    let items = &["Apply", "Skip", "Abort (quit)"];

    let selection = Select::new()
        .with_prompt(format!("Apply {} change(s)?", change_count))
        .items(items)
        .default(0)
        .interact()?;

    Ok(match selection {
        0 => FileAction::Apply,
        1 => FileAction::Skip,
        _ => FileAction::Abort,
    })
}

fn cmd_registry(registry_name: &str, git_ref: Option<&str>, depth: Option<usize>) -> Result<()> {
    let reg = registry::evaluate(registry_name, git_ref)?;
    registry::print_tree(&reg, depth.unwrap_or(usize::MAX), 0);
    Ok(())
}

fn cmd_scan(
    paths: Option<Vec<PathBuf>>,
    exclude: &[String],
    use_default_excludes: bool,
) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);
    let files = scanner::collect_nix_files(&scan_paths, exclude, use_default_excludes)?;

    println!("Would scan {} files:", files.len());
    for file in files {
        println!("  {}", file.display());
    }

    Ok(())
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
        print_broken_ref(broken);
    }
}

fn print_broken_ref(broken: &BrokenRef) {
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
        let reason = broken.reason.as_deref().unwrap_or("no suggestion");
        println!(
            "  {} {} {}",
            loc.dimmed(),
            broken.reference.path.red(),
            format!("({})", reason).dimmed()
        );
    }
}
