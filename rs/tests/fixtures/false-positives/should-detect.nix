# Test cases that SHOULD be detected - the actual imp registry pattern.
# These confirm the tool correctly identifies real registry references.
{ registry, pkgs, lib, ... }:

let
  # Basic registry reference in let binding
  aliceHome = registry.users.alice;
  
  # Registry in list
  profiles = [
    registry.profiles.desktop
    registry.profiles.server
  ];
  
  # Registry in function call argument
  merged = lib.mkMerge [
    registry.modules.base
    registry.modules.networking
  ];
  
  # Registry in conditional
  optionalModule = lib.optionalAttrs (config.desktop.enable) registry.modules.gui;
  
  # Registry in with expression body
  withResult = with lib; mkIf true registry.hosts.server;
  
  # Multi-line attribute access (should still work)
  multiLine = registry
    .deeply
    .nested
    .path;
in {
  imports = [
    # Standard import list usage
    registry.hosts.desktop
    registry.modules.nixos.base
    registry.modules.home.features.git
    
    # Inline with other expressions
    (registry.modules.optional)
    
    # In concatenation context
  ] ++ [ registry.modules.extra ];
  
  # Registry in module option definition
  home.packages = [ registry.packages.custom ];
  
  # Registry as value in attrset
  myConfig = {
    base = registry.configs.base;
    overlay = registry.overlays.custom;
  };
  
  # Deeply nested usage
  programs.neovim.plugins = [
    registry.modules.home.features.neovim.plugins
  ];
}
