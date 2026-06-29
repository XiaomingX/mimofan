# Xiaomi MiMo Token Plan Evidence, 2026-06-23

This folder contains browser-captured notes for issue #2621. The screenshots
and extracted JSON were captured from Xiaomi-owned pages so future provider
catalog updates can compare repo metadata against the live source of truth.

## Sources

- [Xiaomi MiMo model summary](https://mimo.mi.com/docs/en-US/quick-start/summary/model)
- [Xiaomi MiMo pay-as-you-go pricing](https://mimo.mi.com/docs/en-US/price/pay-as-you-go)
- [Xiaomi MiMo Token Plan](https://platform.xiaomimimo.com/token-plan)
- Secondary input: `/Users/hunterbown/Downloads/ai_provider_models_2026_catalog.xlsx`

## Findings

- `mimo-v2.5-pro`, `mimo-v2.5-pro-ultraspeed`, and `mimo-v2.5` are treated as
  1,000,000-token Xiaomi MiMo V2.5 chat models in mimo-tui metadata.
- `mimo-v2-omni` remains a 256K-window V2-series model; mimo-tui does not use
  it as the current `xiaomi-mimo` Omni shorthand.
- Token Plan usage is credit/quota based and is not interoperable with
  pay-as-you-go account balance. mimo-tui therefore leaves direct
  `xiaomi-mimo` cost unknown until Xiaomi exposes a reliable balance endpoint.
- The workbook snapshot listed `mimo-v2.5` as 262,144 tokens, but Xiaomi's
  current official model summary shows the V2.5 chat model at 1,000,000 tokens.
  The official docs win.

## Captures

- `03-xiaomi-model-table.png` / `.json`: official model table and RPM/TPM notes.
- `04-xiaomi-payg-pricing.png` / `.json`: PAYG versus Token Plan separation.
- `05-xiaomi-payg-pricing-table.png`: PAYG pricing table.
- `07-xiaomi-token-plan-public.png` / `.json`: public Token Plan package page.

These files are evidence notes, not a live CI fixture. Automated tests should
assert mimo-tui's bundled metadata and provider behavior; a future docs-refresh
job can re-capture these pages and flag drift for review.
