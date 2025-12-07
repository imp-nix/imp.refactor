# Edge cases: syntactic positions where registry detection is tricky.
# Each case is annotated with expected behavior.
{ registry, lib, config, ... }:

let
  # SHOULD DETECT: registry as base in various expression contexts
  
  # In parentheses
  a = (registry.path.one);
  
  # As function argument
  b = lib.mkDefault registry.path.two;
  
  # In list literal
  c = [ registry.path.three registry.path.four ];
  
  # In attrset value position
  d = { key = registry.path.five; };
  
  # Chained with or
  e = registry.path.six or null;
  
  # With // merge operator
  f = { } // registry.path.seven;
  
  # In interpolation (tricky - string context)
  # g = "${registry.path.eight}"; # This is string interp, may or may not detect
  
  # After rec keyword
  h = rec { val = registry.path.nine; };

  # SHOULD NOT DETECT: registry is not the base identifier
  
  # Accessing .registry on another ident
  i = config.nix.registry.nixpkgs;
  
  # Accessing .registry on a path expression
  j = (import ./foo.nix).registry.bar;
  
  # Let-bound shadowing
  registry' = { shadowed.path = 1; };
  k = registry'.shadowed.path;  # Different ident (registry')
  
  # Inherit brings registry into scope differently
  l = let inherit (inputs) registry; in registry.from.inputs;
  
  # Attribute set with registry as key (definition, not reference)
  m = { registry.as.key = "value"; };
  
  # Function application where registry is called
  n = registry { arg = 1; };  # registry used as function, not attr select
  
  # Prefix match but different ident
  registryBackup = { old.path = 1; };
  o = registryBackup.old.path;
  
in {
  inherit a b c d e f h;
  # These should NOT match
  inherit i j k l m n o;
}
