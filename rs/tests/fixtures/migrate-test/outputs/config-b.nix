# Test file with broken registry references (simulating rename)
{ registry, ... }:
{
  imports = [
    registry.users.alice # broken: was renamed from home.alice
    registry.mods.nixos # broken: was renamed from modules.nixos
  ];
}
