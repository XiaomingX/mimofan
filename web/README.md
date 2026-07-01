# mimofan-web

Community site for [Mimofan](https://github.com/XiaomingX/mimofan) — lives at **mimofan.net**.

Next.js 15 (App Router) + Tailwind, deployed to Cloudflare Workers via [`@opennextjs/cloudflare`](https://opennext.js.org/cloudflare). Curated "Today's Dispatch" content is regenerated every 6 hours by a Cloudflare Cron Trigger that calls `deepseek-v4-flash` to summarise recent repo activity, and stored in Workers KV.

## Local dev

```bash
cd web
pnpm install
cp .env.example .env.local   # fill in the keys you have
pnpm dev                  # http://localhost:3000
```

Env (mirrors `.env.example`):

| Variable                    | What                                                             | Required?            |
| --------------------------- | ---------------------------------------------------------------- | -------------------- |
| `DEEPSEEK_API_KEY`          | DeepSeek platform key (`sk-...`)                                 | only for the `/api/cron` tasks (summarization + community agent) |
| `GITHUB_TOKEN`              | Fine-grained PAT, public-repo read scope                         | optional (raises rate limit 60 → 5000 req/h) |
| `GITHUB_REPO`               | Defaults to `XiaomingX/mimofan`                                   | optional             |
| `CRON_SECRET`               | Shared secret for manual `/api/cron` invocation                  | optional (Cloudflare cron triggers don't need it) |
| `DEEPSEEK_MODEL`            | Defaults to `deepseek-v4-flash`                                  | optional             |
| `DEEPSEEK_BASE_URL`         | Defaults to `https://api.deepseek.com`                           | optional             |
| `MAINTAINER_TOKEN`          | Admin panel auth; access `/admin?token=<value>`                  | only for `/admin`    |
| `MAINTAINER_GITHUB_PAT`     | PAT with `issues:write`, for posting comments via `/admin`       | only for `/admin` posting |
| `NEXT_PUBLIC_GITEE_ENABLED` | Set to `1` once the Gitee mirror exists; blank hides Gitee links | optional             |

The site renders fine without any of them — `Today's Dispatch` falls back to a static editorial; the GitHub feed shows "feed not yet loaded".

## Deploy to Cloudflare

You already own `mimofan.net` on Cloudflare and have a Workers Paid plan. The deploy is two steps:

1. **Provision KV namespaces once:**

   ```bash
   npx wrangler kv namespace create CURATED_KV
   npx wrangler kv namespace create NEXT_INC_CACHE_KV
   ```

   Copy the printed `id` values into the matching `wrangler.jsonc` bindings
   (replace each `REPLACE_WITH_KV_ID`).

2. **Set secrets and deploy:**

   ```bash
   npx wrangler secret put DEEPSEEK_API_KEY
   npx wrangler secret put GITHUB_TOKEN     # optional
   npx wrangler secret put CRON_SECRET      # optional, for manual /api/cron?task=curate hits

   pnpm deploy                           # builds with OpenNext + uploads
   ```

3. **Point the domain:** in the Cloudflare dashboard, add a Worker route for `mimofan.net/*` → the deployed Worker, named `mimofan-web` (see `wrangler.jsonc`).

The first cron run happens within 6 hours; you can also kick it manually:

```bash
curl -H "x-cron-secret: $CRON_SECRET" "https://mimofan.net/api/cron?task=curate"
```

## What's where

Pages are bilingual: each `app/[locale]/` page renders both English and
Chinese from the same file, keyed by the `[locale]` segment (`en` / `zh`,
see `lib/i18n/config.ts`). Copy changes must update both locales.

```
web/
├── app/
│   ├── globals.css             design system: paper grain, hairlines, type, seal
│   ├── [locale]/               en / zh — every page is bilingual
│   │   ├── layout.tsx          root + locale layout: html shell, fonts, nav, footer
│   │   ├── page.tsx            home — hero, dispatch, stats, how-it-works, join
│   │   ├── install/page.tsx    per-OS install with auto-detection
│   │   ├── docs/page.tsx       modes / tools / approval / config / mcp / providers
│   │   ├── faq/page.tsx        frequently asked questions
│   │   ├── feed/page.tsx       live mirror of issues + PRs
│   │   ├── roadmap/page.tsx    shipped / underway / considered / ruled out
│   │   ├── contribute/page.tsx how to PR + house rules + dev loop
│   │   └── admin/              maintainer panel (page.tsx + admin-client.tsx)
│   └── api/
│       ├── cron/route.ts          cron tasks: curate, triage, facts-drift, …
│       ├── github/feed/route.ts   cached JSON endpoint
│       └── admin/                 login, logout, post (MAINTAINER_TOKEN-gated)
├── components/
│   ├── nav.tsx                 sticky header w/ date strip + CJK accents
│   ├── footer.tsx              dense 5-column footer
│   ├── seal.tsx                red Chinese-seal mark used as section anchor
│   ├── ticker.tsx              animated live activity strip
│   ├── stat-grid.tsx           tabular repo stats row
│   ├── feed-card.tsx           one issue/PR card
│   ├── locale-switcher.tsx     EN ↔ ZH toggle
│   └── install-*.tsx           install page blocks (binary, code block, tiles)
├── lib/
│   ├── types.ts                shared types
│   ├── i18n/                   locale config, en/zh dictionaries
│   ├── github.ts               REST client + relative-time formatter
│   ├── deepseek.ts             v4-flash chat client + curate() prompt
│   ├── facts.ts                getFacts(): KV value, else build-time FACTS
│   ├── facts.generated.ts      GENERATED — do not edit by hand
│   ├── facts-drift.ts          runtime re-derivation for the drift cron
│   ├── community-agent.ts      triage / pr-review / digest cron tasks
│   └── kv.ts                   Cloudflare KV access via OpenNext bindings
├── scripts/
│   ├── derive-facts.mjs        prebuild: repo sources → lib/facts.generated.ts
│   └── check-kv-id.mjs         predeploy guard for KV namespace ids
├── wrangler.jsonc              CF Worker config + cron + KV binding
├── open-next.config.ts         OpenNext adapter config
└── tailwind.config.ts          design tokens
```

## Facts pipeline

Mechanical facts (version, provider list, sandbox backends, crate names,
default model, Node engines) are never hand-written into pages:

1. **Build time** — `scripts/derive-facts.mjs` runs as `prebuild` (and before
   `npm run dev`), parses the parent repo (`Cargo.toml`, `crates/tui/src/config.rs`,
   `crates/tui/src/sandbox/`, `npm/mimofan/package.json`) and writes
   `lib/facts.generated.ts`. Never edit that file by hand.
2. **Runtime** — the `/api/cron?task=facts-drift` cron (`lib/facts-drift.ts`)
   re-derives the same facts from `raw.githubusercontent.com` on a schedule and
   writes changes to `CURATED_KV` under `facts:current`. Pages call
   `getFacts()` (`lib/facts.ts`), which prefers the KV value over the
   build-time constant — so a version bump or new provider self-corrects
   within one cron tick, without a redeploy.

When a new `ApiProvider` variant lands in `crates/tui/src/config.rs`, it must
be added to the `labelMap` in **both** `scripts/derive-facts.mjs` and
`lib/facts-drift.ts` (or to the `EXCLUDED` set if deliberately hidden). Both
fail loudly on unmapped variants, so the build / cron will tell you.

## Aesthetic

"Yamen tech": Qing memorial document × WeChat news feed × Bloomberg terminal.

- **Palette**: white paper `#FFFFFF`, ink `#0E0E10`, indigo `#4D6BFE`, aged ochre, jade green, cobalt blue.
- **Type**: Fraunces (display), IBM Plex Sans (body), JetBrains Mono (UI/code), Noto Serif SC (decorative CJK anchors).
- **Structure**: hairline 1px dividers, multi-column grids, big tabular numbers, surgical use of red for "hot" markers, decorative Chinese-seal squares as section anchors.

If you want to retune the palette, edit `:root` in `app/globals.css` and the `colors` block in `tailwind.config.ts`.
