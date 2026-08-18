#!/usr/bin/env python3
"""rd_exec_eval.py — 长程执行 + R&D 自动化质量的统一评分 harness（benchmark/long_horizon）。

把两类 l_level=4 样本统一到一个评分入口：

  A. 长程执行（long_exec.json, source=mettlebench/lhtb）
     1. ordered_checklist_prefix  有序检查表前缀进度（MettleBench 思想：前缀连续满足）
     2. recovery_axis              执行恢复轴（从检查点恢复、不重跑已完成、不破坏不可撤销产物）
     3. order_violating_control    顺序违规对照（避免"先删 legacy 再迁移"这类破坏检查表的顺序错误）

  B. R&D 自动化（rd_automation.json, source=paperbench/researchcodebench）
     1. rubric_tree        paperbench 风格 rubric 树：递归遍历叶子，LLM-as-judge 判是否满足；
                           weighted sum of leaf satisfaction → 0~1
     2. researchcodebench  researchcodebench 风格：target_signature + judge=
                           run_original_unit_tests_pass_at_1，判签名存在且语义正确

复用 p0_dynamic.client_cfg / call_messages 调真模型；judge 走我们自己的端点
（yes/no 二值，仿 longmemeval_harness.judge_hit），不做自由文本打分以保证可复现。

用法：
    # 仅校验评分逻辑（mock 轨迹 + stub judge，无需模型/网络）
    python3 benchmark/long_horizon/rd_exec_eval.py --selftest

    # 真模型端到端评分（需 ANTHROPIC_BASE_URL / ANTHROPIC_MODEL / ANTHROPIC_AUTH_TOKEN）
    python3 benchmark/long_horizon/rd_exec_eval.py --limit 5 --json results/rd_exec.json
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
AGENTBENCH = HERE.parent / "agentbench"
sys.path.insert(0, str(AGENTBENCH))
from p0_dynamic import client_cfg, call_messages  # 复用真模型调用骨架（不重写）

SAMPLES = HERE / "samples"

# 样本文件名
LONG_EXEC_FILE = "long_exec.json"
RD_AUTO_FILE = "rd_automation.json"


# ---------------------------------------------------------------------------
# 样本加载
# ---------------------------------------------------------------------------
def load_samples() -> list[dict]:
    """读 long_exec.json + rd_automation.json，合并 tasks 列表（带 source 标记）。"""
    out: list[dict] = []
    for fname in (LONG_EXEC_FILE, RD_AUTO_FILE):
        p = SAMPLES / fname
        if not p.exists():
            sys.stderr.write(f"[rd_exec_eval] 样本缺失: {p}\n")
            continue
        data = json.loads(p.read_text(encoding="utf-8"))
        family = "long_exec" if fname == LONG_EXEC_FILE else "rd_automation"
        for t in data.get("tasks", []):
            t = dict(t)
            t["_srcfile"] = fname
            t["_family"] = family
            out.append(t)
    return out


# ---------------------------------------------------------------------------
# LLM-as-judge（yes/no 二值，仿 judge_hit）
# ---------------------------------------------------------------------------
def judge(cfg: dict, rubric: str, evidence: str) -> bool | None:
    """judge 返回 'yes' in text.lower() 二值；调用失败返回 None。"""
    prompt = (
        "You are a grader for an agent task. Given a RUBRIC (what good looks like) "
        "and the AGENT EVIDENCE (trace excerpt), answer 'yes' ONLY if the evidence "
        "satisfies the rubric. Respond with a single word: yes or no.\n\n"
        f"RUBRIC: {rubric}\n\n"
        f"AGENT EVIDENCE:\n{evidence[:1500]}\n\n"
        "Grade (yes/no only):"
    )
    r = call_messages(cfg, messages=[{"role": "user", "content": prompt}], max_tokens=256)
    if r.get("error"):
        return None
    return "yes" in r["text"].strip().lower()


# ===========================================================================
# A. 长程执行维度评分
# ===========================================================================
def score_ordered_checklist_prefix(cfg: dict, task: dict, response: str) -> float:
    """有序检查表前缀进度：LLM-as-judge 判 agent 响应是否按序满足 checklist。

    前缀连续满足，断裂即止；返回 satisfied_prefix_length / total（0~1）。
    """
    checklist = task.get("verifier_checklist", [])
    if not checklist:
        return 0.0
    satisfied = 0
    for i, item in enumerate(checklist):
        rubric = (
            f"检查表项 {item.get('id')}（kind={item.get('kind')}）："
            f"{item.get('intent', '')}。\n"
            f"要求：agent 的响应必须已经满足这一项（且它前面的所有项都已满足）。"
        )
        ok = judge(cfg, rubric, response)
        if ok is None:
            # judge 失败：保守地视为未满足，停止前缀。
            break
        if ok:
            satisfied += 1
        else:
            break
    return satisfied / len(checklist)


def score_recovery_axis(cfg: dict, task: dict, response: str) -> bool | None:
    """执行恢复轴：结构化校验 agent 是否展现恢复语义。"""
    ra = task.get("recovery_axis", {})
    expected = ra.get("expected_on_rewind", "")
    if not expected:
        return None
    rubric = (
        f"执行恢复轴期望：{expected}\n"
        f"机制：{ra.get('checkpoint_mechanism', '')}。\n"
        "判断 agent 的响应是否展现了『从最近检查点恢复、不重跑已完成步骤、"
        "不破坏不可撤销产物』的语义。"
    )
    return judge(cfg, rubric, response)


def score_order_violating_control(cfg: dict, task: dict, response: str) -> bool | None:
    """顺序违规对照：judge 判 agent 是否避免了破坏检查表的顺序错误。"""
    ra = task.get("recovery_axis", {})
    control = ra.get("order_violating_control", "")
    if not control:
        return None
    rubric = (
        f"顺序违规控制要求：{control}\n"
        "判断 agent 的响应是否避免了上述顺序错误（即没有先执行会破坏检查表的"
        "前置步骤）。避免了返回 yes，出现了该顺序错误返回 no。"
    )
    return judge(cfg, rubric, response)


# ===========================================================================
# B. R&D 自动化维度评分
# ===========================================================================
def traverse_rubric(node: dict, visit) -> None:
    """递归遍历 rubric 树，对每个节点调用 visit(node)。"""
    visit(node)
    for child in node.get("children", []) or []:
        traverse_rubric(child, visit)


def _collect_leaves(node: dict) -> list[dict]:
    leaves: list[dict] = []
    traverse_rubric(node, lambda n: leaves.append(n) if not n.get("children") else None)
    return leaves


def score_rubric_tree(cfg: dict, task: dict, response: str) -> float | None:
    """paperbench 风格 rubric 树：递归遍历叶子，judge 判是否满足 criteria。

    聚合：weighted sum of leaf satisfaction（叶子权重相对叶子总权重归一化）→ 0~1。
    """
    root = task.get("rubric")
    if not root:
        return None
    leaves = _collect_leaves(root)
    if not leaves:
        return None
    total_w = 0.0
    satisfied_w = 0.0
    for leaf in leaves:
        w = float(leaf.get("weight", 1.0))
        total_w += w
        criteria = leaf.get("criteria", "")
        tests = leaf.get("tests", "")
        rubric = (
            f"叶子要求：{leaf.get('description', '')}\n"
            f"判定标准：{criteria}\n"
            f"关联测试：{tests}\n"
            "判断 agent 生成的代码/响应是否满足该叶子要求（标准与测试均满足才算 yes）。"
        )
        ok = judge(cfg, rubric, response)
        if ok is None:
            continue  # judge 失败则不计该叶子
        if ok:
            satisfied_w += w
    if total_w == 0.0:
        return 0.0
    return satisfied_w / total_w


def score_researchcodebench(cfg: dict, task: dict, response: str) -> bool | None:
    """researchcodebench 风格：target_signature + judge=run_original_unit_tests_pass_at_1。

    selftest 用 stub；真模型模式可让模型跑单测，这里用 judge 判签名存在且语义正确。
    """
    sig = task.get("target_signature")
    judge_kind = task.get("judge")
    if not sig or judge_kind != "run_original_unit_tests_pass_at_1":
        return None
    rubric = (
        f"目标函数签名：{sig}\n"
        "判断 agent 生成的代码是否包含该正确签名，且函数语义正确"
        "（可被原单元测试通过）。"
    )
    return judge(cfg, rubric, response)


# ===========================================================================
# 真模型端到端：构造 prompt → 调模型 → 按维度判定 → 汇总
# ===========================================================================
def run_one(cfg: dict, task: dict) -> dict:
    instr = task.get("instruction") or task.get("title") or ""
    r = call_messages(cfg, messages=[{"role": "user", "content": instr}], max_tokens=2048)
    response = "" if r.get("error") else r.get("text", "")

    rec: dict = {"id": task.get("id"), "family": task.get("_family")}
    if task.get("_family") == "long_exec":
        rec["ordered_checklist_prefix"] = round(
            score_ordered_checklist_prefix(cfg, task, response), 3)
        rec["recovery_axis"] = score_recovery_axis(cfg, task, response)
        rec["order_violating_control"] = score_order_violating_control(cfg, task, response)
    else:  # rd_automation
        rec["rubric_tree"] = (
            round(score_rubric_tree(cfg, task, response), 3)
            if task.get("rubric") else None)
        rec["researchcodebench"] = score_researchcodebench(cfg, task, response)
    return rec


def run(cfg: dict, limit: int | None) -> dict:
    samples = load_samples()
    if limit:
        samples = samples[:limit]
    per = [run_one(cfg, s) for s in samples]
    return {"summary": {"samples": len(per)}, "per_sample": per}


# ===========================================================================
# --selftest：内置 mock task + mock agent 响应，stub judge 验证评分逻辑
# ===========================================================================
def _make_stub_judge(table: dict):
    """返回一个 stub judge：根据 (rubric, evidence) 定向判定二值结果。

    table 的 value 为 2 元组 (rubric_frag, bool)：
      - 命中条件：rubric_frag 出现在 rubric 中 且 key 出现在 evidence 中
        （rubric_frag 为空字符串 "" 表示只按 evidence 子串判定，用于
        RECOVER_* / ORDER_* 这类仅作 evidence 标记、不在 rubric 出现的场景）。
    这样同一段 evidence 含 'leaf1_ok' 时，只有 rubric 含对应 rubric_frag
    的那个叶子会被判 True，避免跨叶子误匹配。

    回退规则：rubric 含 'Pxx' 检查表项 id（长程执行前缀进度场景），取决于
    evidence 是否含 'Pxx_done'。均未命中返回 False（保守未满足）。
    """
    import re
    def stub(cfg, rubric, evidence):
        # 字面表
        for key, spec in table.items():
            if key in evidence:
                frag, val = spec if isinstance(spec, tuple) else (key, spec)
                if frag == "" or frag in rubric:
                    return val
        # 回退：rubric 含 'Pxx' 检查表项 id（长程执行前缀进度场景），
        # 取决于 evidence 是否含 'Pxx_done'。
        m = re.search(r"P\d{2}", rubric)
        if m:
            return f"{m.group(0)}_done" in evidence
        return False
    return stub


def _selftest() -> int:
    # --- mock 长程执行 task ---
    mock_le = {
        "id": "LE-MOCK",
        "_family": "long_exec",
        "verifier_checklist": [
            {"id": "P01", "kind": "file_exists", "intent": "a"},
            {"id": "P02", "kind": "no_forbidden_change", "intent": "b"},
            {"id": "P03", "kind": "file_content", "intent": "c"},
            {"id": "P04", "kind": "test", "intent": "d"},
        ],
        "recovery_axis": {
            "checkpoint_mechanism": "snap",
            "order_violating_control": "先删 legacy 再迁移则 P02 失败",
            "expected_on_rewind": "从检查点恢复不重跑",
        },
    }
    # 前缀进度：满足前 3 项 → 0.75。evidence 含 P01/P02/P03 标记，第 4 项标记缺失。
    ev_prefix = "P01_done P02_done P03_done"  # 不含 P04
    # recovery_axis：正确响应 vs 错误响应
    ev_recover_ok = "RECOVER_OK 从检查点恢复不重跑且未破坏产品"
    ev_recover_bad = "RECOVER_BAD 从零重跑所有步骤，破坏了不可撤销产物"
    # order_violating：正确（避免）vs 错误（出现顺序错误）
    ev_order_ok = "ORDER_OK 先迁移后处理 legacy，未破坏检查表"
    ev_order_bad = "ORDER_BAD 先删 legacy 再迁移，导致 P02 失败"

    # --- mock R&D task（paperbench 风格）---
    mock_rd = {
        "id": "RD-MOCK",
        "_family": "rd_automation",
        "rubric": {
            "description": "root", "weight": 1.0,
            "children": [
                {"description": "leaf1", "criteria": "c1", "weight": 0.5, "tests": "t1"},
                {"description": "leaf2", "criteria": "c2", "weight": 0.5, "tests": "t2"},
            ],
        },
        "target_signature": "def apply_rope(q, k, cos, sin):",
        "judge": "run_original_unit_tests_pass_at_1",
    }
    ev_rubric_both = "leaf1_ok leaf2_ok"      # 两叶子都满足 → 1.0
    ev_rubric_one = "leaf1_ok"                # 仅 1 叶 → 0.5
    ev_sig_ok = "def apply_rope(q, k, cos, sin): correct semantics"
    ev_sig_bad = "def wrong_name(x): ..."     # 无目标签名

    # 用 stub judge 替换全局 judge，按 evidence 子串定向判定。
    global judge
    orig = judge
    # value = (rubric_frag, bool)：rubric_frag 空串表示仅按 evidence 标记判定；
    # 叶子类要求 rubric 含对应 leaf 描述以区分（"leaf1_ok" 只会命中 leaf1 的判定）。
    judge = _make_stub_judge({
        "RECOVER_OK": ("", True),
        "RECOVER_BAD": ("", False),
        "ORDER_OK": ("", True),
        "ORDER_BAD": ("", False),
        "leaf1_ok": ("leaf1", True),
        "leaf2_ok": ("leaf2", True),
        "def apply_rope(q, k, cos, sin):": ("def apply_rope(q, k, cos, sin):", True),
    })
    try:
        # 1) 长程执行前缀进度：4 项满足前 3 → 0.75
        sc_prefix = score_ordered_checklist_prefix({}, mock_le, ev_prefix)
        assert abs(sc_prefix - 0.75) < 1e-6, f"prefix 应为 0.75，实得 {sc_prefix}"
        # 全满足应 =1.0，全不满足=0.0
        assert abs(score_ordered_checklist_prefix({}, mock_le,
                  "P01_done P02_done P03_done P04_done") - 1.0) < 1e-6
        assert abs(score_ordered_checklist_prefix({}, mock_le, "nothing") - 0.0) < 1e-6

        # 2) recovery_axis：正确响应 True，错误响应 False
        assert score_recovery_axis({}, mock_le, ev_recover_ok) is True
        assert score_recovery_axis({}, mock_le, ev_recover_bad) is False

        # 3) order_violating_control：避免 True，出现错误 False
        assert score_order_violating_control({}, mock_le, ev_order_ok) is True
        assert score_order_violating_control({}, mock_le, ev_order_bad) is False

        # 4) rubric 树遍历：2 叶子权重 0.5+0.5，全满足→1.0，满足1→0.5
        assert abs(score_rubric_tree({}, mock_rd, ev_rubric_both) - 1.0) < 1e-6
        assert abs(score_rubric_tree({}, mock_rd, ev_rubric_one) - 0.5) < 1e-6
        # 嵌套权重归一化：RD-002 风格（4 叶子：0.3+0.3+0.2+0.2）
        # 内部节点 c(0.2) 只作分组，叶子权重即其份额（c1=0.2）。
        # 叶子描述用 LA/LB/LC1/LD 这类无歧义 token，避免与 rubric 中
        # "agent" 等词发生子串误匹配（单字符 a/b/d 会命中 "agent"）。
        nested = {
            "description": "root", "weight": 1.0,
            "children": [
                {"description": "LA", "weight": 0.3},
                {"description": "LB", "weight": 0.3},
                {"description": "LC", "weight": 0.2, "children": [
                    {"description": "LC1", "weight": 0.2}]},
                {"description": "LD", "weight": 0.2},
            ],
        }
        judge_multi = _make_stub_judge({
            "LVA": ("LA", True), "LVB": ("LB", True),
            "LVC1": ("LC1", True), "LVD": ("LD", False)})
        g = judge
        judge = judge_multi
        # 满足 LA/LB/LC1（0.3+0.3+0.2=0.8），LD 不满足 → 0.8
        sc = score_rubric_tree({}, {"rubric": nested}, "LVA LVB LVC1")
        assert abs(sc - 0.8) < 1e-6, f"嵌套权重应为 0.8，实得 {sc}"
        judge = g

        # 5) researchcodebench 签名判定：有签名 True，无签名 False
        assert score_researchcodebench({}, mock_rd, ev_sig_ok) is True
        assert score_researchcodebench({}, mock_rd, ev_sig_bad) is False
    finally:
        judge = orig

    print("[selftest] 全部通过")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true", help="mock 校验评分逻辑，不调模型")
    ap.add_argument("--limit", type=int, default=None, help="最多评测样本数（需环境变量）")
    ap.add_argument("--json", dest="json_out", default=None, help="结果输出 JSON 路径")
    args = ap.parse_args()

    if args.selftest:
        return _selftest()

    # 真模型模式：缺失环境变量则由 client_cfg 报错退出（sys.exit(2)）。
    cfg = client_cfg()
    result = run(cfg, args.limit)
    txt = json.dumps(result, ensure_ascii=False, indent=2)
    if args.json_out:
        Path(args.json_out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.json_out).write_text(txt, encoding="utf-8")
        print(f"[rd_exec_eval] 结果已写入 {args.json_out}")
    else:
        print(txt)
    return 0


if __name__ == "__main__":
    sys.exit(main())
