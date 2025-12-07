# imp.refactor

Detect and fix broken registry references in Nix projects. When you rename a directory in your imp registry, this tool finds all `registry.old.path` references in your codebase and rewrites them to `registry.new.path`.

## The Problem

After renaming `nix/registry/home/` to `nix/registry/users/`, every file that references `registry.home.alice` breaks. Finding and fixing these manually is tedious and error-prone. The pure-Nix approach in imp.lib's `migrate.nix` cannot work because Nix flake evaluation copies everything to the store before it runs, meaning both the scanned files and the registry reflect the same (post-rename) state.

imp.refactor solves this by operating directly on your working directory while evaluating the registry from git HEAD.

## Usage

```sh
# Scan working tree, show broken refs with suggestions
nix run github:imp-nix/imp.refactor -- detect

# Same, but with explicit rename hints
nix run github:imp-nix/imp.refactor -- detect --rename home=users

# Preview what would change
nix run github:imp-nix/imp.refactor -- apply

# Actually rewrite files
nix run github:imp-nix/imp.refactor -- apply --write
```

### Commands

**detect** scans `.nix` files for `registry.X.Y.Z` patterns, validates each path against the evaluated registry, and reports broken references. For each broken ref, it attempts to suggest a replacement using either an explicit rename map or a leaf-name heuristic (finds paths ending with the same final segment).

```sh
imp-refactor detect --paths ./nix/outputs --verbose
imp-refactor detect --rename home=users --rename svc=services
imp-refactor detect --json  # machine-readable output
```

**apply** rewrites broken references in-place. Without `--write`, it shows a diff of proposed changes.

```sh
imp-refactor apply              # dry-run
imp-refactor apply --write      # modify files
```

**registry** displays the current registry structure for debugging.

```sh
imp-refactor registry
imp-refactor registry --depth 2
```

**scan** shows which files would be scanned, useful for verifying path configuration.

## How It Works

The tool runs in four stages:

1. Walk the working directory (not store paths) collecting `.nix` files, skipping directories prefixed with `.` or `_`.

1. Parse each file with `rnix` and extract attribute access chains that start with the registry name. Unlike regex, this correctly handles multi-line expressions, comments, and string literals.

1. Evaluate `nix eval --json .#registry` to get the current registry structure, then flatten it into a set of valid dotted paths.

1. Compare extracted references against valid paths. Broken refs get suggestions via rename map (longest-prefix-wins) or leaf-name heuristic (unique suffix match).

## Rename Map

When the leaf-name heuristic fails (ambiguous matches or actual leaf renames), provide explicit mappings:

```sh
imp-refactor detect --rename home=users --rename "svc.db=services.database"
```

The `--rename` flag accepts `old=new` pairs. Longer prefixes take precedence, so `--rename home=users --rename home.alice=admins.alice` correctly maps `home.alice.settings` to `admins.alice.settings`.

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
