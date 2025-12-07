# Multiple different renames in one file
{ registry, ... }:
{
  imports = [
    # home.alice.programs -> users.alice.programs (parent dir renamed)
    registry.home.alice.programs.editor
    registry.home.bob.shell

    # svc -> services (parent dir renamed)
    registry.svc.database.postgresql
    registry.svc.web.nginx

    # mods.profiles -> profiles (nested moved up)
    registry.mods.profiles.desktop.gnome
  ];
}
