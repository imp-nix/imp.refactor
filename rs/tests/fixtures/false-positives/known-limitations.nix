# Known limitation: function parameter shadowing.
# The scanner cannot distinguish between the global imp registry
# and a local parameter named `registry`. This is a static analysis
# limitation - we'd need semantic analysis to track scopes.
#
# These cases WILL be detected as registry refs even though they
# reference a local parameter, not the imp registry.
{ ... }:

let
  # Function with registry as a parameter - the body will match
  mkService = { registry, port }: {
    url = registry.endpoint;  # WILL MATCH: registry.endpoint
    config = registry.settings.base;  # WILL MATCH: registry.settings.base
  };
  
  # Lambda with registry parameter
  processRegistry = registry: registry.data.items;  # WILL MATCH: registry.data.items
  
  # Nested function with shadowing
  outer = { registry, ... }: {
    inner = registry.nested.value;  # WILL MATCH
  };
in {
  # These all produce matches even though they're not imp registry refs
  service = mkService { registry = { endpoint = "http://localhost"; }; port = 8080; };
}
