---
name: security-audit
description: Run a structured security audit of the current workspace — SAST via semgrep (SARIF), dependency/OSV advisories, and taint/gadget-chain review. Use when the user asks for a security review, wants to find vulnerabilities, or before shipping sensitive code.
metadata:
  short-description: Security audit via semgrep, OSV, and taint analysis
---

# Security Audit

Use this skill to drive a thorough, evidence-first security review of a
workspace. Prefer automated signals over eyeballing.

## When to use

- The user asks "audit this repo", "find vulnerabilities", "security review".
- Before shipping code that handles external input, secrets, or serialization.
- As the analysis step of `/code-review` when the security persona is active.

## Workflow

1. **Static SAST with semgrep.** Produce SARIF so the reviewer can normalize it
   into `security_issues`:
   ```bash
   semgrep --config auto --sarif --output semgrep.sarif .
   ```
   If `semgrep` is unavailable, fall back to the bundled rule presets and the
   static analyzer's taint engine. Never claim a clean bill of health you
   didn't actually verify.

2. **Dependency / OSV scan.** Parse the lockfile (`Cargo.lock`,
   `package-lock.json`) and cross-check against the OSV advisory database;
   prune to reachable dependencies only.

3. **Gadget-chain & knowledge-base check.** Enumerate attack surface: which
   known gadget chains (Log4Shell, C3P0, Commons-Collections, Fastjson
   autoType) are satisfiable given the resolved dependencies.

4. **Taint review.** For each source→sink path the analyzer reports, confirm
   the evidence chain and whether a sanitizer (strong or partial) breaks it.

5. **Report.** Emit one `security_issues` entry per confirmed finding with
   `severity`, `category`, `title`, `description`, `path`, `line`, and the
   `rule_id` / evidence chain. Separate confirmed from suspected. Name the tool
   that produced each signal.

## Safety

- Audit is read-only. Do not auto-fix unless the user explicitly asks; if you
  do, run the test suite and be ready to roll back.
- Do not exfiltrate secrets you find. Report their location; redact values.
- Treat semgrep configs and third-party rules as untrusted input.
