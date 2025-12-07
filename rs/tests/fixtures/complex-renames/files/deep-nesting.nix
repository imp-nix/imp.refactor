# Deep nesting with renames at various levels
{ registry, ... }:
{
  imports = [
    # svc.db -> services.database
    registry.svc.db.postgresql
    registry.svc.db.redis

    # svc.http -> services.web
    registry.svc.http.nginx
    registry.svc.http.caddy

    # utils.helpers -> lib.helpers
    registry.utils.helpers.strings
  ];
}
