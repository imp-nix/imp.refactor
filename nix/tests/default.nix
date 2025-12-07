/**
  Unit tests for imp-refactor nix lib.

  Run with: nix flake check
*/
{ lib }:
let
  refactorLib = import ../lib.nix { inherit lib; };
in
{
  # flattenRegistryPaths tests
  flattenRegistryPaths."test empty" = {
    expr = refactorLib.flattenRegistryPaths "" { };
    expected = [ ];
  };

  flattenRegistryPaths."test single level" = {
    expr = refactorLib.flattenRegistryPaths "" {
      alice = { };
      bob = { };
    };
    expected = [
      "alice"
      "bob"
    ];
  };

  flattenRegistryPaths."test nested" = {
    expr = builtins.sort builtins.lessThan (
      refactorLib.flattenRegistryPaths "" {
        home = {
          alice = { };
          bob = { };
        };
      }
    );
    expected = [
      "home"
      "home.alice"
      "home.bob"
    ];
  };

  # applyRenameMap tests
  applyRenameMap."test exact match" = {
    expr = refactorLib.applyRenameMap { home = "users"; } "home";
    expected = "users";
  };

  applyRenameMap."test prefix match" = {
    expr = refactorLib.applyRenameMap { home = "users"; } "home.alice";
    expected = "users.alice";
  };

  applyRenameMap."test no match returns null" = {
    expr = refactorLib.applyRenameMap { home = "users"; } "other.path";
    expected = null;
  };

  applyRenameMap."test longest prefix wins" = {
    expr = refactorLib.applyRenameMap {
      home = "users";
      "home.alice" = "admins.alice";
    } "home.alice.settings";
    expected = "admins.alice.settings";
  };

  # suggestByLeaf tests
  suggestByLeaf."test unique match" = {
    expr = refactorLib.suggestByLeaf [
      "users.alice"
      "users.bob"
    ] "home.alice";
    expected = "users.alice";
  };

  suggestByLeaf."test ambiguous returns null" = {
    expr = refactorLib.suggestByLeaf [
      "users.alice"
      "admins.alice"
    ] "home.alice";
    expected = null;
  };

  suggestByLeaf."test no match returns null" = {
    expr = refactorLib.suggestByLeaf [
      "users.bob"
      "users.carol"
    ] "home.alice";
    expected = null;
  };
}
