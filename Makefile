# mimofan 评测/数据闭环统一入口（薄壳，转调 benchmark/run_all.py）
#
# 用法：
#   make bench-selftest  快速自检（accept + harness selftest + model-cmp verify）
#   make bench           全量离线评测（accept + harness --limit/--skip-exec，不触真模型付费）
#   make bench-live      真模型全量（full + 真模型 harness 全量 + model-cmp run）
#   make accept          仅发布前验收 shell
#   make registry        重建 sample_registry（含 score 回灌闭环）
#   make snapshot        打时间戳快照
#   make help            列出所有 target

PYTHON ?= python3

.PHONY: help bench bench-selftest bench-live accept registry snapshot

help:
	@echo "mimofan 评测/数据闭环入口"
	@echo "  make bench-selftest   快速自检（fast 档，不触真模型）"
	@echo "  make bench            全量离线评测（full 档，默认）"
	@echo "  make bench-live       真模型全量（live 档）"
	@echo "  make accept           仅发布前验收 shell"
	@echo "  make registry         重建 sample_registry（带回灌 score）"
	@echo "  make snapshot         打时间戳快照"

bench:
	$(PYTHON) benchmark/run_all.py --full

bench-selftest:
	$(PYTHON) benchmark/run_all.py --fast

bench-live:
	$(PYTHON) benchmark/run_all.py --live

accept:
	$(PYTHON) benchmark/run_all.py --group accept

registry:
	$(PYTHON) benchmark/build_sample_registry.py

snapshot:
	$(PYTHON) benchmark/snapshot.py
