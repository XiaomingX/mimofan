# WhaleFlow Authoring

WhaleFlow has one runtime boundary: authored workflow source lowers to typed
Rust `WorkflowSpec`, Rust validates the IR, and the scheduler/headless worker
runtime executes leaves. Authoring languages do not get hidden authority to own
files, shell, network, providers, cancellation, or TUI state.

## Language Choice

| Surface | Strength | Tradeoff | v0.8.60 stance |
|---|---|---|---|
| YAML / JSON IR | Simple, reviewable, no runtime | Verbose for generated workflows | Keep as interchange/debug format |
| Starlark | Existing safe evaluator and helper functions | Less familiar to most JS/TS developers and coding agents | Keep supported |
| JavaScript | Familiar object syntax and easy agent generation | Unsafe if executed as a general runtime | First-class authoring through declarative compile-only subset |
| TypeScript | Best editor/types story for workflow SDK | Needs stripping/typechecking if full TS is supported | Same compile-only subset for now; richer SDK later |

The default high-capability path is TypeScript/JavaScript authoring, but only as
a compile step. The v0.8.60 compiler accepts a JSON-compatible object inside
`workflow({...})` from `.workflow.js` or `.workflow.ts`, lowers it to
`WorkflowSpec`, and runs the same Rust validation gate used by Starlark.

## Contract

Accepted source shape:

```js
export default workflow({
  "id": "issue-audit-js",
  "goal": "Audit an issue fix with parallel agents",
  "nodes": [
    {
      "branch": {
        "id": "parallel-audit",
        "children": [
          { "agent": { "id": "code-audit", "prompt": "Review code", "agent_type": "review" } },
          { "agent": { "id": "test-audit", "prompt": "Review tests", "agent_type": "verifier" } }
        ]
      }
    },
    { "reduce": { "id": "summary", "inputs": ["code-audit", "test-audit"], "prompt": "Summarize" } }
  ]
});
```

Supported node wrappers: `agent`, `branch`, `sequence`, `reduce`,
`teacher_review`, `loop_until`, `cond`, and `expand`. Raw `WorkflowNode` JSON IR
with `kind` / `spec` also remains valid.

The compiler rejects effectful constructs such as `import`, `require`, `fetch`,
`process`, `Deno`, `Bun`, `child_process`, file reads/writes, `eval`, `async`,
and `await`. This is intentionally stricter than JavaScript: workflow source is
a familiar declaration format, not a second execution runtime.

## Verification

- `cargo test -p mimo-tui-whaleflow --locked javascript`
- `cargo test -p mimo-tui-whaleflow --locked starlark`

Current example: `workflows/issue_audit.workflow.js`.
