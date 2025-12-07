# Test file with registry references
{ registry, ... }:
{
  imports = [
    registry.home.alice # valid reference
    registry.modules.nixos # valid reference
  ];
}
