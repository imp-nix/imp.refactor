# Nested renames - parent renamed affects all children
{ registry, ... }:
{
  imports = [
    # All these are broken because home -> users
    registry.home.alice.programs.editor
    registry.home.alice.programs.zsh
    registry.home.bob.shell
  ];
}
