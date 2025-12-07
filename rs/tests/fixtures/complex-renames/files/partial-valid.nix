# Mix of valid and broken refs
{ registry, ... }:
{
  imports = [
    # Valid refs (in current registry)
    registry.users.alice.programs.editor
    registry.services.database.postgresql
    registry.profiles.desktop.gnome

    # Broken refs (old names)
    registry.home.bob.shell
    registry.svc.web.caddy
  ];
}
