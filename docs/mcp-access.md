# MCP read capabilities

The core parses `[mcp] presets`, `[mcp] allow_paths` and optional positive `max_input_bytes` (default 16 MiB). Both allowlists default empty. Preset names are distinct simple identifiers; paths must be nonempty and resolved entries distinct. The [MCP stdio interface](mcp.md) uses these held capabilities for every file read, including preset file templates.

Path entries can grant one regular file or a directory subtree. Startup resolves trusted configuration paths and holds read-only descriptors. Tool path inputs resolve relative to the server working directory or use absolute paths; their resolved paths must fit an allowed capability. Internal symlinks can resolve to an allowed file. Escaping symlinks, parent traversal outside the scope and sibling names sharing a string prefix are refused.

Directory reads walk the resolved relative path from the held directory descriptor, with NOFOLLOW on every opened component. A changed symlink cannot redirect the walk outside the capability. If the configured anchor's named inode changes, reads refuse and require a server restart. Explicit single-file grants hold that original inode. Final objects must be regular single-link files; directories, FIFOs and hardlinks are refused. Opens never create, truncate, append, lock for writing or execute anything.

Each read uses positional I/O, so repeated or concurrent reads of an explicit file grant have independent offsets. Input size is checked before and during the read, including growing files; UTF-8 is required. Individual filesystem reads are not a snapshot against concurrent content writers. Capabilities bound the objects read, rather than promising immutable bytes.
