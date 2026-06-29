## Model Characteristics

**Prefix-cache hygiene.** Many providers cache shared prompt prefixes. Prefer appending to existing messages over mutating old ones — deletion or replacement can break the cache and increase cost. Structure output to maximize prefix reuse across turns.

**Parallel execution.** Batch independent reads, searches, and greps into a single turn. Never serialize operations that can run concurrently — parallel tool calls share the same turn and finish faster.
