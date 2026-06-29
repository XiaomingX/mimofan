## Context Purge

Free space in the conversation's context window. Below is the current history with stable numeric IDs. Identify content that is clearly no longer needed for the ongoing work.

### Operations

remove  — Delete an entire message by its ID. Example:
          {"op": "remove", "msg": 3}

replace — Rewrite part of a specific content block using regex substitution.
          pattern uses Rust regex syntax. Must specify both `block` and
          `pattern` and `with`. Example:
          {"op": "replace", "msg": 7, "block": 0,
           "pattern": "read \\d+ files", "with": "read files"}

### Pairing rule

Every ToolUse block is paired with its ToolResult. If you remove a message
containing a tool call, its result will be removed too — and vice versa. You
do not need to list both.

### What to keep

- Important decisions, architectural choices
- File paths that are still relevant
- Tool outputs that contain information not yet acted upon

### What to prune

- Verbose tool outputs whose information has been fully consumed
- Redundant confirmations ("done", "ok", "that worked")
- Superseded file reads (the file was later written/modified)
- Boilerplate that the model already incorporated into later work

Be conservative. When in doubt, keep the message.

### Conversation
