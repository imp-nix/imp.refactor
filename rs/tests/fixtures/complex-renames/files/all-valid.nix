# All valid - should not produce any suggestions
{ registry, ... }:
{
  imports = [
    registry.users.alice.programs.editor
    registry.users.alice.programs.zsh
    registry.users.bob.shell
    registry.services.database.postgresql
    registry.services.web.nginx
    registry.profiles.desktop.gnome
    registry.lib.helpers.strings
  ];
}
