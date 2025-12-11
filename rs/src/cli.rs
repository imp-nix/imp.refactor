//! Command-line interface definitions.
//!
//! Defines the argument parser and subcommands using clap's derive API.
//! Each subcommand corresponds to a distinct operation: detecting broken
//! references, applying fixes, inspecting the registry, or listing scan targets.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Detect and fix broken registry references in Nix projects.
#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan files and report broken registry references with suggested fixes.
    Detect {
        /// Paths to scan. Defaults to current directory.
        #[arg(short, long)]
        paths: Option<Vec<PathBuf>>,

        /// Glob patterns for directories/files to exclude (e.g., "node_modules", "*.generated.nix").
        /// By default, entries starting with `.` or `_` are excluded.
        #[arg(short, long)]
        exclude: Vec<String>,

        /// Disable default exclusion of `.` and `_` prefixed entries.
        #[arg(long)]
        no_default_excludes: bool,

        /// Registry attribute name in flake outputs.
        #[arg(long, default_value = "registry")]
        registry_name: String,

        /// Git ref to evaluate registry from (e.g., HEAD, HEAD^, main).
        /// Compares working tree files against registry at this ref.
        #[arg(long)]
        git_ref: Option<String>,

        /// Explicit rename mappings in `old=new` format. Longest prefix wins.
        #[arg(long, value_parser = parse_rename)]
        rename: Vec<(String, String)>,

        /// Emit JSON instead of human-readable output.
        #[arg(long)]
        json: bool,

        /// Print additional diagnostics to stderr.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Apply suggested renames to files.
    Apply {
        /// Interactively confirm each file's changes before applying.
        #[arg(short, long)]
        interactive: bool,

        /// Paths to scan. Defaults to current directory.
        #[arg(short, long)]
        paths: Option<Vec<PathBuf>>,

        /// Glob patterns for directories/files to exclude (e.g., "node_modules", "*.generated.nix").
        /// By default, entries starting with `.` or `_` are excluded.
        #[arg(short, long)]
        exclude: Vec<String>,

        /// Disable default exclusion of `.` and `_` prefixed entries.
        #[arg(long)]
        no_default_excludes: bool,

        /// Registry attribute name in flake outputs.
        #[arg(long, default_value = "registry")]
        registry_name: String,

        /// Git ref to evaluate registry from (e.g., HEAD, HEAD^, main).
        #[arg(long)]
        git_ref: Option<String>,

        /// Explicit rename mappings in `old=new` format.
        #[arg(long, value_parser = parse_rename)]
        rename: Vec<(String, String)>,
    },

    /// Print the registry's attribute tree.
    Registry {
        /// Registry attribute name in flake outputs.
        #[arg(long, default_value = "registry")]
        registry_name: String,

        /// Git ref to evaluate registry from (e.g., HEAD, HEAD^, main).
        #[arg(long)]
        git_ref: Option<String>,

        /// Maximum tree depth to display.
        #[arg(long)]
        depth: Option<usize>,
    },

    /// List files that would be scanned without processing them.
    Scan {
        /// Paths to scan. Defaults to current directory.
        #[arg(short, long)]
        paths: Option<Vec<PathBuf>>,

        /// Glob patterns for directories/files to exclude (e.g., "node_modules", "*.generated.nix").
        /// By default, entries starting with `.` or `_` are excluded.
        #[arg(short, long)]
        exclude: Vec<String>,

        /// Disable default exclusion of `.` and `_` prefixed entries.
        #[arg(long)]
        no_default_excludes: bool,
    },
}

fn parse_rename(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid rename format '{}', expected 'old=new'", s));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}
