# mimofan Skill Audit & Inventory List

This document lists all active and available skills within the **mimofan** project, categorized by their usage scope. This is generated for user audit to decide which skills to retain or remove.

---

## 🛠️ User-Facing Built-in Skills
*These skills are bundled inside the `mimofan` TUI assets (`crates/tui/assets/skills/`) and shipped to end-users as product features.*

| Skill Name | Path / Location | Description | Audit Recommendation | Rationale |
|:---|:---|:---|:---|:---|
| **v4-best-practices** | [SKILL.md](crates/tui/assets/skills/v4-best-practices/SKILL.md) | Use when working with deepseek-v4-pro or deepseek-v4-flash in thinking mode on multi-step or plan-driven tasks. Provides rules to prevent stale references, unverified plan assumptions, and vague plan output. | `Keep or Refactor (待定)` | V4 specific best practices. Might need updating to Next.js 16/Rust 1.88 standards. |
| **fleet-manager** | [SKILL.md](crates/tui/assets/skills/fleet-manager/SKILL.md) | Use when managing, triaging, restarting, escalating, or summarizing mimofan Agent Fleet runs and workers. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **feishu** | [SKILL.md](crates/tui/assets/skills/feishu/SKILL.md) | Work with Feishu or Lark bots, docs, sheets, bitables, approval flows, and OpenAPI/MCP setup without hardcoding credentials. | `Keep (保留)` | Feishu/Lark integration, key messaging connector. |
| **spreadsheets** | [SKILL.md](crates/tui/assets/skills/spreadsheets/SKILL.md) | Create, edit, analyze, clean, or convert spreadsheets including XLSX, CSV, TSV, formulas, charts, and tabular reports. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **pdf** | [SKILL.md](crates/tui/assets/skills/pdf/SKILL.md) | Read, extract, split, merge, rotate, watermark, fill, OCR, or create PDF files with verification of page counts and text extraction. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **delegate** | [SKILL.md](crates/tui/assets/skills/delegate/SKILL.md) | Strategic delegation for multi-step coding, research, or verification work. Use when a task can be split into parent reasoning plus focused sub-agent execution through the agent tool. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **skill-creator** | [SKILL.md](crates/tui/assets/skills/skill-creator/SKILL.md) | Create or improve mimofan skills. Use when the user wants a new skill, wants to update an existing skill, or needs guidance on when a skill should be a skill versus MCP, hooks, tools, or a plugin scaffold. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **presentations** | [SKILL.md](crates/tui/assets/skills/presentations/SKILL.md) | Create, edit, inspect, or convert PowerPoint decks and PPTX slide presentations with practical layout and verification steps. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **plugin-creator** | [SKILL.md](crates/tui/assets/skills/plugin-creator/SKILL.md) | Scaffold mimofan local plugin directories and activation notes. Use when the user asks to create, package, or sketch a plugin for mimofan. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **skill-installer** | [SKILL.md](crates/tui/assets/skills/skill-installer/SKILL.md) | Install, update, trust, or inspect DeepSeek skills from GitHub or local skill folders. Use when the user asks for available skills or wants a community skill installed. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **documents** | [SKILL.md](crates/tui/assets/skills/documents/SKILL.md) | Create, edit, inspect, or convert Word documents and DOCX deliverables such as memos, reports, letters, templates, and forms. | `Keep (保留)` | Provides built-in user features for file-handling, automation, or integrations. |
| **mcp-builder** | [SKILL.md](crates/tui/assets/skills/mcp-builder/SKILL.md) | Design, build, configure, or debug Model Context Protocol servers for mimofan, including stdio and HTTP/SSE transports. | `Keep (保留)` | Helps TUI users generate Model Context Protocol configurations. |

---

## 🧑‍💻 Maintainer & DevOps Stewardship Skills
*These workflows are stored under `docs/skills/` and are used by AI agents/developers to maintain mimofan's GitHub issues, pull requests, credit harvests, and release checks.*

| Skill Name | Path / Location | Description | Audit Recommendation | Rationale |
|:---|:---|:---|:---|:---|
| **gh-find-prs** | [SKILL.md](docs/skills/gh-find-prs/SKILL.md) | Survey open mimofan PRs and triage each for mergeability and disposition against the real landing branch. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |
| **codew-release-qa-sweep** | [SKILL.md](docs/skills/codew-release-qa-sweep/SKILL.md) | Use before claiming mimofan release work is done: run the full gate sweep and list the manual QA targets. | `Keep (保留)` | Release QA sweep tool used for ensuring release lane stability. |
| **gh-treasure-hunt** | [SKILL.md](docs/skills/gh-treasure-hunt/SKILL.md) | Hunt the issue/PR queue for highest value-over-risk wins: clean focused community PRs, already-implemented issues to close, safe quick-fixes. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |
| **gh-compile-issues** | [SKILL.md](docs/skills/gh-compile-issues/SKILL.md) | Triage N GitHub issues into a coverage matrix: fetch each, check current code, classify already-done/quick-fix/design/defer with cited evidence. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |
| **gh-file-issue** | [SKILL.md](docs/skills/gh-file-issue/SKILL.md) | Use when filing a new mimofan GitHub issue: turn a bug or idea into a well-formed, actionable issue with repro, acceptance criteria, labels, and milestone. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |
| **gh-assign-issues** | [SKILL.md](docs/skills/gh-assign-issues/SKILL.md) | Use to assign GitHub issues to a milestone and/or owners in bulk, verifying each. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |
| **gh-close-issues** | [SKILL.md](docs/skills/gh-close-issues/SKILL.md) | Close resolved mimofan issues only after verifying the landed commit/behavior, with a positive crediting comment; never from title alone. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |
| **gh-credit-harvest** | [SKILL.md](docs/skills/gh-credit-harvest/SKILL.md) | Harvest one community PR into a release branch with authorship and credit preserved, verified green, and a warm thank-you. | `Keep (保留)` | Automates harvests of Co-Authored-By credits from community PRs. |
| **gh-plan-issues** | [SKILL.md](docs/skills/gh-plan-issues/SKILL.md) | Cluster a milestone of issues into coherent implementation workstreams with sequencing, dependencies, and a lead train. | `Keep (保留)` | Crucial for automated GitHub project management and issue triaging. |

---

## 📝 Audit Decision & Next Steps
Please audit the above list. To remove a skill:
1. Delete its physical directory (`rm -rf <path_to_skill>`).
2. (For TUI built-in skills) Unregister the skill definition from `crates/tui/src/skills/system.rs`.

---
*Generated automatically by Antigravity AI on 2026-06-30.*
