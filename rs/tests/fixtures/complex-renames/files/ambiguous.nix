# Ambiguous renames - same leaf name, multiple possible targets
{ registry, ... }:
{
  imports = [
    # "editor" only exists in one place, should match
    registry.old.programs.editor

    # "gnome" only exists in one place, should match
    registry.old.desktop.gnome

    # "minimal" only exists in one place, should match
    registry.config.server.minimal

    # "base" doesn't exist anywhere - no match
    registry.configs.base
  ];
}
