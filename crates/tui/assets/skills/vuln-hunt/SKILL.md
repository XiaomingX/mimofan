---
name: vuln-hunt
description: Long-horizon security workflow to FIND AND REPRODUCE a vulnerability in a real project (e.g. fastjson deserialization gadget, Spring Boot RCE) using hypothesis/evidence tracking, gadget-chain tracing, and a realized run_poc.
---

# Vuln Hunt (long-horizon security analysis)

Use this skill when the task is to find AND reproduce a vulnerability in a
real project (e.g. fastjson deserialization gadget, Spring Boot RCE), not to
merely classify code.

## Workflow (follow in order; do not skip steps)
1. Recon: enumerate attack surface — deps, entry points, sinks
   (call_graph, ast_query, security_audit/semgrep).
2. Hypothesize: for each suspected sink, register a hypothesis with
   `hypothesis action=create` stating the claimed exploit class + CVE.
3. Trace: run `gadget_chain_trace` for the sink; identify which gadgets are
   PRESENT vs MISSING in the target — the missing ones are what you must prove
   reachable or rule out. Cross-check with call_graph reachability.
4. Gather evidence: for every hypothesis, call `hypothesis action=add_evidence`
   with concrete code references / taint paths BEFORE concluding.
5. Reproduce: build a candidate PoC and run `run_poc` with an `expect` string
   that proves the vulnerable behavior. realized=true => exploit confirmed.
6. Verdict: `hypothesis action=resolve verdict=confirmed|refuted` — this is
   REFUSED unless evidence was attached (consistency gate). Never assert a
   finding without evidence + a realized PoC.

## Consistency rules (what gets scored)
- Every claim must have a registered hypothesis + >=1 evidence entry.
- A hypothesis with zero evidence CANNOT be resolved.
- A "confirmed" verdict without a realized run_poc is invalid.
- Prefer reconstructing the attack PRINCIPLE (why it works) over a bare PoC.

## Tooling reference
- hypothesis: create / add_evidence / resolve / list
- gadget_chain_trace: sink + present_gadgets -> chain satisfied/missing
- run_poc: command + expect -> realized bool (executed in sandbox)
- call_graph / ast_query / security_audit: static traceability
