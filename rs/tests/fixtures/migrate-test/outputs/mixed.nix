# Test file with mix of valid and broken references
{ registry, ... }:
{
  imports = [
    registry.hosts.server # valid
    registry.users.bob # broken: was renamed from home.bob
  ];
}
