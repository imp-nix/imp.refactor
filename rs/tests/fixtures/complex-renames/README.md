# Complex Renames Test Fixtures

Test fixtures for migration functionality that covers:

- Multiple simultaneous directory renames
- Nested renames (parent change affects all descendants)
- Deep path changes (4+ levels)
- Leaf-based matching with unique/ambiguous/missing cases
- Mixed valid and broken refs in single file
- Multi-directory scanning

## Structure

```
complex-renames/
├── registry/          # "After" state - current valid paths
│   ├── users/alice/programs/{editor,zsh}.nix
│   ├── users/bob/shell.nix
│   ├── services/{database,web}/*.nix
│   ├── profiles/{desktop,server}/*.nix
│   └── lib/helpers/strings.nix
└── files/             # Files with "old" (broken) references
    ├── multi-rename.nix      # home->users, svc->services, mods.profiles->profiles
    ├── nested-rename.nix     # Parent rename affects all children
    ├── deep-nesting.nix      # svc.db->services.database, utils->lib
    ├── ambiguous.nix         # Unique leaf matching + no-match cases
    ├── partial-valid.nix     # Mix of valid and broken refs
    └── all-valid.nix         # Control: all refs are valid
```

## Issues Encountered when Designing Fixtures

### 1. Registry treats directories with `default.nix` as leaf modules

**Problem**: Initially created directories like `users/default.nix` alongside `users/alice/`. The registry treats any directory containing `default.nix` as a single module path, ignoring children.

**Technical cause**: In `src/registry.nix:79-83`:

```nix
if hasDefault then
  # Directory with default.nix is a single module
  { ${attrName} = path; }
```

**Workaround**: Registry directories that need children must NOT have `default.nix`. Only leaf directories should have `default.nix`.

**User impact**: This is intentional behavior. If you have `users/default.nix`, then `registry.users` is a module - you don't separately reference `registry.users.alice`. Either:

- Use `users/default.nix` (no children exposed in registry)
- Use `users/alice/default.nix` without `users/default.nix` (children exposed)

### 2. Test file naming collision with existing filter test

**Problem**: Named a file `neovim.nix` which ends in `m.nix`. An existing test in `tests/core.nix` filters `./fixtures` for files matching `*m.nix`, causing unexpected matches.

**Workaround**: Renamed to `editor.nix` to avoid collision.

**User impact**: None. This is purely a test isolation issue.

### 3. `suggestNewPath` matches on leaf name only

**Technical detail**: The migration tool matches broken refs to valid paths by comparing the final path segment (leaf). For `home.alice.programs.neovim` -> the leaf is `neovim`.

**Implications**:

- Unique leaf names get correct suggestions
- Ambiguous leaves (same name in multiple locations) return `null` - no automatic fix
- Non-existent leaves return `null`

**User impact**: After complex renames, some refs may not auto-suggest if the leaf name is ambiguous. Users will see these in the "broken refs" output but must fix manually.
