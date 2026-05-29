import re

with open('apps/linux/src/main.rs', 'r') as f:
    code = f.read()

# Replace core retrieval with api/facade
# For the bulk of the methods (workspace, project, volume, chapter, settings, sync)
# They are very compatible.
def replace_common_borrow(match):
    return "if let Some(core) = self.core_api() {"

# 1. Update `if let Some(core_ref) = &self.core { let core = core_ref.borrow();` to use core_api()
code = re.sub(r'if let Some\(core_ref\) = &self\.core\s*\{\s*let core = core_ref\.borrow\(\);\s*', 'if let Some(core) = self.core_api() {\n            ', code)

# Same for `borrow_mut()` which might have been used? (we checked, no mut used on core directly)

# 2. But we need to use core_facade() for methods not in WriterCoreApi yet!
legacy_methods = [
    "bind_mind_map_node_to_anchor",
    "create_mind_map_anchor",
    "create_mind_map_edge",
    "create_mind_map_graph",
    "create_mind_map_node",
    "delete_mind_map_edge",
    "delete_mind_map_node",
    "list_mind_map_graphs",
    "set_default_mind_map_graph",
    "update_mind_map_edge",
    "update_mind_map_node",
    "execute_action",
    "read_chapter",
]

# Find blocks calling legacy methods and revert them to core_facade
for block in code.split('if let Some(core) = self.core_api() {'):
    # This is too fragile to parse this way.
    pass

