//! imp-refactor library for detecting and fixing broken registry references.
//!
//! This library provides programmatic access to the registry refactoring
//! functionality. The core workflow involves three phases:
//!
//! 1. **Scanning**: Collect `.nix` files and extract `registry.X.Y.Z` references
//! 2. **Analysis**: Compare references against valid registry paths and generate suggestions
//! 3. **Rewriting**: Apply fixes to source files
//!
//! # Example
//!
//! ```no_run
//! use imp_refactor::{scanner, registry, analyzer};
//! use std::collections::HashMap;
//! use std::path::PathBuf;
//!
//! // Collect files and extract references
//! let files = scanner::collect_nix_files(&[PathBuf::from("./nix")], &[], true).unwrap();
//! let mut refs = Vec::new();
//! for file in &files {
//!     refs.extend(scanner::extract_registry_refs(file, "registry").unwrap());
//! }
//!
//! // Evaluate the registry (None = current working tree, Some("HEAD") = committed state)
//! let reg = registry::evaluate("registry", None).unwrap();
//! let valid_paths = registry::flatten_paths(&reg, "");
//!
//! // Analyze references
//! let rename_map = HashMap::new();
//! let (broken, valid_count) = analyzer::analyze(&refs, &valid_paths, &rename_map);
//!
//! println!("Found {} broken references", broken.len());
//! ```

pub mod analyzer;
pub mod registry;
pub mod rewriter;
pub mod scanner;

// Re-export commonly used types at crate root
pub use analyzer::{BrokenRef, DetectionResult, Diagnostics};
pub use scanner::RegistryRef;
