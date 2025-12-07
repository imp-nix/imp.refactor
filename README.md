# imp.refactor

Detect and fix broken registry references in Nix projects.

The [imp registry](https://github.com/imp-nix/imp.lib) maps directory structure to attribute paths: `registry/users/alice/default.nix` becomes `registry.users.alice`. When you rename `registry/home/` to `registry/users/`, every `registry.home.alice` reference breaks. This tool scans your `.nix` files for broken references and rewrites them.

A pure-Nix solution cannot work here. Flake evaluation copies everything to the store before code runs, so the scanned files and registry always reflect the same committed state. imp.refactor operates directly on working tree files while evaluating the registry from a git ref, allowing it to detect drift between the two.

## Usage

```sh
nix run github:imp-nix/imp.refactor -- detect                    # find broken refs
nix run github:imp-nix/imp.refactor -- detect --rename home=users # explicit rename hints
nix run github:imp-nix/imp.refactor -- apply                     # preview changes
nix run github:imp-nix/imp.refactor -- apply --write             # rewrite files
nix run github:imp-nix/imp.refactor -- apply --interactive       # confirm each file
```

The `detect` command scans `.nix` files for `registry.X.Y.Z` patterns using `rnix` AST parsing (not regex), validates each against `nix eval .#registry`, and reports broken references. For each broken ref, it suggests a replacement via an explicit rename map or a leaf-name heuristic that matches the final path segment.

```sh
imp-refactor detect --paths ./nix/outputs --verbose
imp-refactor detect --rename home=users --rename svc=services
imp-refactor detect --json
```

The `apply` command rewrites broken references. Without `--write`, it shows a unified diff of proposed changes. With `--interactive`, it prompts for confirmation before modifying each file.

```sh
imp-refactor apply                          # dry-run
imp-refactor apply --write                  # modify files
imp-refactor apply --interactive            # per-file prompts
imp-refactor apply --git-ref HEAD^ --write  # compare against previous commit
```

The `registry` command displays the current registry structure for debugging. The `scan` command lists which files would be scanned.

## Internals

The scanner walks the working directory collecting `.nix` files. By default, entries starting with `.` or `_` are skipped. Use `--exclude` to add glob patterns, or `--no-default-excludes` to disable the defaults entirely:

```sh
imp-refactor detect --exclude "node_modules" --exclude "*.generated.nix"
imp-refactor detect --no-default-excludes  # scan everything
```

The tool runs in four stages:

1. Walk directories collecting `.nix` files, filtering by exclude patterns.
1. Parse each file with `rnix` and extract attribute access chains starting with the registry name. AST parsing correctly handles multi-line expressions, comments, and string literals.
1. Evaluate `nix eval --json .#registry` to get the registry structure, then flatten it into a set of valid dotted paths.
1. Compare extracted references against valid paths. Broken refs get suggestions via rename map (longest prefix wins) or leaf-name heuristic (unique suffix match).

## Rename maps

When the leaf-name heuristic fails (ambiguous matches or actual leaf renames), provide explicit mappings:

```sh
imp-refactor detect --rename home=users --rename "svc.db=services.database"
```

Longer prefixes take precedence, so `--rename home=users --rename home.alice=admins.alice` maps `home.alice.settings` to `admins.alice.settings` rather than `users.alice.settings`.

## Development

```sh
nix build             # build the CLI
nix flake check       # run all checks (formatting, clippy, nix-unit)
nix fmt               # format everything
nix develop           # shell with Rust toolchain

cd rs && cargo test   # run Rust tests
```

## License

[GPL-3.0](LICENSE)
