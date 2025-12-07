/**
  Nix helper functions for imp-refactor.

  These functions assist with registry path analysis and can be used
  for integration testing or as utilities from Nix code.
*/
{ lib }:
let
  inherit (lib)
    filter
    flatten
    hasPrefix
    hasSuffix
    mapAttrsToList
    sort
    stringLength
    unique
    ;

  /**
    Recursively flatten a registry attrset into a list of dotted path strings.

    # Arguments

    - `prefix` (string): Current path prefix (start with "")
    - `attrs` (attrset): Registry or sub-attrset to flatten

    # Returns

    List of strings like ["home.alice" "home.bob" "modules.desktop"]
  */
  flattenRegistryPaths =
    prefix: attrs:
    flatten (
      mapAttrsToList (
        name: value:
        let
          path = if prefix == "" then name else "${prefix}.${name}";
        in
        if builtins.isAttrs value && !(value ? __functor) then
          [ path ] ++ (flattenRegistryPaths path value)
        else
          [ path ]
      ) attrs
    );

  /**
    Apply a rename map to transform an old path to a new path.

    The rename map uses prefix matching with longest-prefix-wins semantics.
    For example, with renameMap = { "home" = "users"; }, the path "home.alice"
    becomes "users.alice".

    # Arguments

    - `renameMap` (attrset): Mapping of old prefixes to new prefixes
    - `oldPath` (string): The path to transform

    # Returns

    The transformed path, or null if no mapping applies.
  */
  applyRenameMap =
    renameMap: oldPath:
    let
      prefixes = builtins.attrNames renameMap;
      sortedPrefixes = sort (a: b: stringLength a > stringLength b) prefixes;

      findMatch =
        remaining:
        if remaining == [ ] then
          null
        else
          let
            prefix = builtins.head remaining;
            rest = builtins.tail remaining;
          in
          if oldPath == prefix then
            renameMap.${prefix}
          else if hasPrefix "${prefix}." oldPath then
            let
              suffix = builtins.substring (stringLength prefix + 1) (stringLength oldPath) oldPath;
            in
            "${renameMap.${prefix}}.${suffix}"
          else
            findMatch rest;
    in
    findMatch sortedPrefixes;

  /**
    Suggest a new path for a broken reference using leaf-name heuristic.

    Searches valid paths for one that ends with the same leaf name.
    Returns null if zero or multiple matches exist (ambiguous).

    # Arguments

    - `validPaths` (list): List of valid dotted path strings
    - `oldPath` (string): The broken path to find a suggestion for

    # Returns

    A suggested path string, or null if no unique match.
  */
  suggestByLeaf =
    validPaths: oldPath:
    let
      parts = lib.splitString "." oldPath;
      leaf = lib.last parts;
      candidates = filter (p: hasSuffix ".${leaf}" p || p == leaf) validPaths;
    in
    if builtins.length candidates == 1 then builtins.head candidates else null;

in
{
  inherit
    flattenRegistryPaths
    applyRenameMap
    suggestByLeaf
    ;
}
