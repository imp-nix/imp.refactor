//! CLI entrypoint for imp-refactor.
//!
//! This binary wraps the imp_refactor library to provide a command-line
//! interface for detecting and fixing broken registry references.

mod cli;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
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

    let files = scanner::collect_nix_files(&scan_paths)?;
    if verbose {
        eprintln!(
            "{} Found {} .nix files to scan",
            "info:".blue().bold(),
            files.len()
        );
    }

    let reg = registry::evaluate(registry_name)?;
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
    paths: Option<Vec<PathBuf>>,
    registry_name: &str,
    rename_map: Vec<(String, String)>,
) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);
    let files = scanner::collect_nix_files(&scan_paths)?;
    let reg = registry::evaluate(registry_name)?;
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

    for (file, changes) in &changes_by_file {
        let action = if write { "Updating:" } else { "Would update:" };
        println!("\n{} {}", action.yellow().bold(), file.display());

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
            rewriter::apply_changes(file, registry_name, changes)?;
        }
    }

    if !write {
        println!("\n{} Use --write to apply changes", "hint:".cyan().bold());
    }

    Ok(())
}

fn cmd_registry(registry_name: &str, depth: Option<usize>) -> Result<()> {
    let reg = registry::evaluate(registry_name)?;
    registry::print_tree(&reg, depth.unwrap_or(usize::MAX), 0);
    Ok(())
}

fn cmd_scan(paths: Option<Vec<PathBuf>>) -> Result<()> {
    let scan_paths = paths.unwrap_or_else(|| vec![PathBuf::from(".")]);
    let files = scanner::collect_nix_files(&scan_paths)?;

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
