# Test cases that should NOT be detected as registry references.
# These are patterns where "registry" appears but is not the imp registry.
{
  pkgs,
  lib,
  config,
  ...
}:

let
  # String containing "registry" - should be ignored
  npmRegistry = "https://registry.npmjs.org";

  # Attribute named "registry" in a different context
  dockerConfig = {
    registry = "ghcr.io";
  };

  # Variable named differently that has .registry attr
  someModule.registry.path = "/var/lib/registry";

  # Let binding that shadows registry
  localRegistry = {
    home.alice = "local";
  };
  result = localRegistry.home.alice;

  # Nested attrset with registry key (not a select from registry ident)
  settings = {
    container.registry.url = "docker.io";
    container.registry.auth.token = "secret";
  };

  # inherit pattern - registry is being inherited, not selected from
  inherit (inputs) registry;

  # Attribute path on left side of assignment
  # (these are definitions, not references)
in
{
  # Accessing config.registry (not bare registry)
  nixpkgs.config.registry = { };

  # nix.registry is a real NixOS option - not our registry
  nix.registry.nixpkgs.flake = pkgs.path;
  nix.registry.home-manager.flake = inputs.home-manager;

  # services.*.registry patterns from real NixOS modules
  services.dockerRegistry.enable = true;
  services.dockerRegistry.listenAddress = "0.0.0.0";

  # virtualisation.docker.registry - not our registry
  virtualisation.docker.autoPrune.enable = true;

  # Quoted attribute access (dynamic) - should be ignored
  foo = registry."home.alice";
  bar = registry.${"home"};

  # Method call on registry (if it were a function)
  # baz = registry { inherit home; };
}
