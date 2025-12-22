# JSON Formatting Optimization Design

## Problem Statement

`serde_json::to_string_pretty()` adds CPU overhead for large JSON responses due to:
- Whitespace insertion (spaces, newlines)
- Additional string allocations
- Increased output size requiring more I/O

For data-heavy endpoints returning hundreds or thousands of entries, this overhead is unnecessary since MCP clients parse JSON programmatically.

## Current State

**36 occurrences of `to_string_pretty` in `src/mcp/server.rs`:**

| Line | Function | Data Size | Recommendation |
|------|----------|-----------|----------------|
| 402 | `fs_get_tree_impl` | Large (up to 10,000 tree entries) | **COMPACT** |
| 440 | `fs_read_script_impl` | Small (single script) | Keep pretty |
| 515 | `fs_write_script_impl` | Small (write result) | Keep pretty |
| 875 | `studio_get_datamodel_impl` | Medium (DataModel tree) | **COMPACT** |
| 914 | `studio_get_datamodel_paginated_impl` | Medium (paginated) | **COMPACT** |
| 941 | `studio_get_script_source_impl` | Small (script source) | Keep pretty |
| 972 | `studio_modify_script_impl` | Small (modification result) | Keep pretty |
| 1012 | `studio_create_instance_impl` | Small (creation result) | Keep pretty |
| 1040 | `studio_delete_instance_impl` | Small (deletion result) | Keep pretty |
| 1074 | `studio_set_property_impl` | Small (property result) | Keep pretty |
| 1102 | `studio_get_properties_impl` | Small (properties) | Keep pretty |
| 1137 | `studio_find_instances_impl` | Large (all instances of class) | **COMPACT** |
| 1174 | `studio_get_selection_impl` | Small (selection) | Keep pretty |
| 1210 | `studio_get_output_impl` | Medium (log entries) | **COMPACT** |
| 1242 | `studio_get_bounds_impl` | Small (bounds) | Keep pretty |
| 1276 | `studio_health_check_impl` | Small (health) | Keep pretty |
| 1306 | `stylua_format_impl` | Small (format result) | Keep pretty |
| 1340 | `rojo_build_impl` | Small (build result) | Keep pretty |
| 1385 | `rojo_sourcemap_impl` | Large (sourcemap JSON) | **COMPACT** |
| 1423 | `wally_install_impl` | Small (install result) | Keep pretty |
| 1463 | `wally_update_impl` | Small (update result) | Keep pretty |
| 1498 | `moonwave_build_impl` | Small (build result) | Keep pretty |
| 1547 | `cloud_publish_place_impl` | Small (publish result) | Keep pretty |
| 1585 | `cloud_upload_asset_impl` | Small (upload result) | Keep pretty |
| 1624 | `cloud_datastore_get_impl` | Small-Medium (single entry) | Keep pretty |
| 1662 | `cloud_datastore_set_impl` | Small (set result) | Keep pretty |
| 1697 | `cloud_restart_servers_impl` | Small (restart result) | Keep pretty |
| 1744 | `cloud_get_universe_impl` | Small (universe info) | Keep pretty |
| 1784 | `cloud_messaging_publish_impl` | Small (publish result) | Keep pretty |
| 1843 | `cloud_ordered_datastore_list_impl` | Large (leaderboard entries) | **COMPACT** |
| 1882 | `cloud_ordered_datastore_set_impl` | Small (set result) | Keep pretty |
| 1922 | `cloud_ordered_datastore_increment_impl` | Small (increment result) | Keep pretty |
| 1959 | `cloud_ordered_datastore_delete_impl` | Small (delete result) | Keep pretty |
| 1996 | `fs_lint_script_impl` | Small-Medium (lint results) | Keep pretty |
| 2036 | `fs_watch_changes_impl` | Medium (file changes) | **COMPACT** |
| 2053 | `server_get_metrics_impl` | Medium (metrics snapshot) | **COMPACT** |

**Note:** `fs_search_content_impl` (line 695) already uses `response.to_string()` (compact).

## Solution

### Endpoints to Change (10 total)

Change from `to_string_pretty()` to `to_string()`:

1. **`fs_get_tree_impl`** (line 402) - Tree can have 10,000 entries
2. **`studio_get_datamodel_impl`** (line 875) - Full DataModel hierarchy
3. **`studio_get_datamodel_paginated_impl`** (line 914) - Paginated DataModel
4. **`studio_find_instances_impl`** (line 1137) - All instances of a class
5. **`studio_get_output_impl`** (line 1210) - Many log entries
6. **`rojo_sourcemap_impl`** (line 1385) - Large sourcemap JSON
7. **`cloud_ordered_datastore_list_impl`** (line 1843) - Leaderboard entries
8. **`fs_watch_changes_impl`** (line 2036) - File change events
9. **`server_get_metrics_impl`** (line 2053) - Metrics data
10. **`fs_get_changes_impl`** - Already compact (uses `json!().to_string()`)

### Implementation

For each endpoint, change:
```rust
// Before
let json = serde_json::to_string_pretty(&result)
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

// After
let json = serde_json::to_string(&result)
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
```

### Benefits

1. **Reduced CPU usage** - No whitespace formatting overhead
2. **Smaller payloads** - Less data to transmit over STDIO
3. **Faster serialization** - Direct JSON output without formatting pass
4. **Consistent behavior** - Aligns with `fs_search_content_impl` which already uses compact

### Trade-offs

- **Readability in logs** - Compact JSON is harder to read in debug output
  - Mitigation: Use JSON formatting tools when debugging
- **No user-facing impact** - MCP clients parse JSON; formatting is irrelevant

## Testing

No test changes required - tests verify JSON structure, not formatting.

Run existing test suite:
```bash
cargo test
```

## Rollout

Single commit changing all 9 endpoints (excluding `fs_get_changes_impl` which is already compact).
