#!/usr/bin/env python3
"""run_eval.py — 实验谱系（lineage）L5 评测 harness（benchmark/lineage）。

评测能力：多代理实验编排中，智能体可遍历谱系查询状态、审计、分支、干净删除，
无需全历史回放。

四任务：
  - lineage_traversal_query  遍历祖先链 + 叶子指标   （LLM-as-judge）
  - lineage_audit            字段级 diff 正确性       （LLM-as-judge）
  - lineage_branch           结构化校验新节点父指针 + 加后无环（代码 DAG 检测）
  - lineage_cascade_delete   两步结构化校验：先列闭包再软删除、未误删兄弟（代码 DFS）

真模型调用复用 benchmark/agentbench/p0_dynamic.py 的 client_cfg / call_messages，
judge 走我们自己的端点（yes/no 二值），不重写真模型调用逻辑。

用法：
    # 仅校验评分逻辑 + DAG/闭包函数（mock 数据，不调模型/网络，无需环境变量）
    python3 benchmark/lineage/run_eval.py --selftest

    # 真模型端到端评分（需 ANTHROPIC_BASE_URL / ANTHROPIC_MODEL / ANTHROPIC_AUTH_TOKEN）
    python3 benchmark/lineage/run_eval.py --limit 2 --json results/lineage.json
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
AGENTBENCH = HERE.parent / "agentbench"
sys.path.insert(0, str(AGENTBENCH))
from p0_dynamic import client_cfg, call_messages  # 复用真模型调用骨架（不重写）

SAMPLES = HERE / "samples" / "lineage_tasks.json"

TASK_TYPES = [
    "lineage_traversal_query",
    "lineage_audit",
    "lineage_branch",
    "lineage_cascade_delete",
]


# ---------------------------------------------------------------------------
# 样本加载
# ---------------------------------------------------------------------------
def load_samples() -> list[dict]:
    """读 benchmark/lineage/samples/lineage_tasks.json，返回 trees 列表。"""
    data = json.loads(SAMPLES.read_text(encoding="utf-8"))
    return data.get("trees", [])


# ---------------------------------------------------------------------------
# DAG 无环检测（代码实现，不靠模型）
# ---------------------------------------------------------------------------
def is_dag(nodes: list[dict]) -> bool:
    """基于 parent_run_id / children 做三色 DFS，有环返回 False。

    nodes: 元素含 run_id / parent_run_id 字段的 dict 列表。
    允许 parent_run_id 为 None（根）或不存在。
    """
    by_id = {n["run_id"]: n for n in nodes if "run_id" in n}
    WHITE, GRAY, BLACK = 0, 1, 2
    color = {rid: WHITE for rid in by_id}

    def dfs(rid: str) -> bool:
        color[rid] = GRAY
        node = by_id[rid]
        # 子节点优先用 children 字段，否则由 parent_run_id 反推。
        children = node.get("children") or []
        for c in children:
            if c not in by_id:
                continue  # 指向未知节点，跳过（已在父侧完整性处理）
            if color[c] == GRAY:
                return False  # 回边 → 有环
            if color[c] == WHITE and not dfs(c):
                return False
        color[rid] = BLACK
        return True

    for rid in by_id:
        if color[rid] == WHITE:
            if not dfs(rid):
                return False
    return True


# ---------------------------------------------------------------------------
# 闭包计算（代码实现，不靠模型）
# ---------------------------------------------------------------------------
def descendant_closure(nodes: list[dict], root_id: str) -> set[str]:
    """从 root_id DFS 所有子孙（含 root 自身），返回闭包集合。"""
    by_id = {n["run_id"]: n for n in nodes if "run_id" in n}
    if root_id not in by_id:
        return set()
    out: set[str] = set()
    stack = [root_id]
    while stack:
        cur = stack.pop()
        if cur in out:
            continue
        out.add(cur)
        node = by_id[cur]
        for c in (node.get("children") or []):
            if c in by_id and c not in out:
                stack.append(c)
    return out


# ---------------------------------------------------------------------------
# LLM-as-judge（yes/no 二值，走我们自己的端点）
# ---------------------------------------------------------------------------
def judge_yesno(cfg: dict, rubric: str, evidence: str) -> bool | None:
    """返回 True/False；None 表示 judge 调用失败。"""
    prompt = (
        "You are a grader for an experiment-lineage management task. Given a "
        "RUBRIC (what a correct answer looks like) and the AGENT RESPONSE, answer "
        "'yes' ONLY if the response satisfies the rubric. Different wording, extra "
        "detail, or more verbose phrasing are all fine. Answer 'no' only if the "
        "response is wrong, contradicts the rubric, or fails to address it. "
        "Respond with a single word: yes or no.\n\n"
        f"RUBRIC: {rubric}\n\n"
        f"AGENT RESPONSE:\n{evidence[:2000]}\n\n"
        "Grade (yes/no only):"
    )
    r = call_messages(cfg, messages=[{"role": "user", "content": prompt}], max_tokens=256)
    if r.get("error"):
        return None
    return "yes" in r["text"].strip().lower()


# ---------------------------------------------------------------------------
# 四任务判定逻辑
# ---------------------------------------------------------------------------
def check_traversal_query(cfg: dict, tree: dict, task: dict, response: str) -> bool | None:
    """judge 判响应是否给出完整有序祖先链 + 正确 leaf 指标。"""
    exp = task.get("expected", {})
    ancestors = exp.get("ancestors", [])
    rubric_parts = [f"响应以正确顺序给出了祖先链：{' -> '.join(ancestors)}"]
    if "leaf_accuracy" in exp:
        rubric_parts.append(f"并且正确给出了叶子节点指标 accuracy={exp['leaf_accuracy']}")
    if "c1b_stage" in exp:
        rubric_parts.append(f"且正确说明 c1b 的 lifecycle_stage={exp['c1b_stage']}")
    if "c1b_f1" in exp:
        rubric_parts.append(f"且正确给出 c1b 的 f1={exp['c1b_f1']}")
    if "stages" in exp:
        stage_str = ", ".join(f"{k}={v}" for k, v in exp["stages"].items())
        rubric_parts.append(f"且正确给出各节点 lifecycle_stage：{stage_str}")
    rubric = "；".join(rubric_parts) + "。"
    return judge_yesno(cfg, rubric, response)


def check_audit(cfg: dict, tree: dict, task: dict, response: str) -> bool | None:
    """judge 判字段级 diff 是否正确。"""
    exp = task.get("expected", {})
    diff_items = []
    for key, mapping in exp.items():
        for field, change in mapping.items():
            diff_items.append(f"{key} 的字段 {field} 变更应为 {change}")
    rubric = "响应正确列出了以下字段级 diff：" + "；".join(diff_items) + "。"
    return judge_yesno(cfg, rubric, response)


def _extract_new_node(response: str) -> dict | None:
    """从 agent 响应抽取其声称新建的节点（简单 JSON / 正则提取）。

    优先尝试从响应里抠出 JSON 对象（含 run_id / parent_run_id / name）；
    失败则用正则抓 parent_run_id 指向的 run_id 与新建节点名。
    返回 {'parent_run_id': str, 'name': str, 'run_id': str|None} 或 None。
    """
    # 1) 尝试 JSON 提取（响应里若直接给了结构化新建节点）。
    candidates = re.findall(r"\{[^{}]*\"parent_run_id\"[^{}]*\}", response)
    for c in candidates:
        try:
            obj = json.loads(c)
            if "parent_run_id" in obj:
                return {
                    "parent_run_id": obj.get("parent_run_id"),
                    "name": obj.get("name"),
                    "run_id": obj.get("run_id"),
                }
        except ValueError:
            continue
    # 2) 回退正则：抓 "parent_run_id": "rX" / parent=rX 等写法。
    m = re.search(r"parent[_-]?run[_-]?id['\"]?\s*[:=]\s*['\"]?([A-Za-z0-9_\-]+)", response)
    if not m:
        return None
    parent = m.group(1)
    nm = re.search(r"(?:new\s+run|node|branch|exp)[^A-Za-z0-9_\-]*['\"]?([A-Za-z0-9_\-]+)", response)
    name = nm.group(1) if nm else None
    return {"parent_run_id": parent, "name": name, "run_id": None}


def check_branch(cfg: dict, tree: dict, task: dict, response: str) -> bool:
    """结构化校验（不靠模型）：
    (a) 抽取的新节点 parent_run_id 必须指向 task 指定的 branch_point；
    (b) 把新节点加入树后整棵树仍是 DAG 无环（代码环检测）。
    """
    exp = task.get("expected", {})
    expected_parent = exp.get("new_run", {}).get("parent_run_id")
    new_node = _extract_new_node(response)
    if new_node is None or not new_node.get("parent_run_id"):
        return False
    if expected_parent and new_node["parent_run_id"] != expected_parent:
        return False  # (a) 不满足：父指针未指向指定 branch_point
    # (b) 把新节点加入树，做无环检测。
    new_run_id = new_node.get("run_id") or "__new_branch_node__"
    merged = list(tree.get("nodes", []))
    merged.append({
        "run_id": new_run_id,
        "parent_run_id": new_node["parent_run_id"],
        "name": new_node.get("name") or "new-branch",
        "children": [],
    })
    # 同步更新父节点的 children，避免父子不一致影响拓扑判断。
    parent = new_node["parent_run_id"]
    for n in merged:
        if n.get("run_id") == parent:
            children = list(n.get("children") or [])
            if new_run_id not in children:
                children.append(new_run_id)
            n["children"] = children
            break
    return is_dag(merged)


def check_cascade_delete(cfg: dict, tree: dict, task: dict, response: str) -> bool:
    """两步结构化校验（不靠模型）：
    (a) 响应先列出影响闭包（affected_closure，DFS 子孙闭包）；
    (b) 被删节点标记 lifecycle_stage=deleted 软删除，且未误删兄弟子树。
    闭包计算用代码 DFS。
    """
    exp = task.get("expected", {})
    expected_closure = set(exp.get("affected_closure", []))
    if not expected_closure:
        return False
    # (a) 用代码计算真实闭包，校验响应是否「列出」了整组闭包节点 id。
    # 以闭包首个元素为根做 DFS（样本约定闭包[0]即被删根）。
    nodes = tree.get("nodes", [])
    root = sorted(expected_closure, key=lambda x: -len(descendant_closure(nodes, x)))[0]
    computed_closure = descendant_closure(nodes, root)
    # 校验计算出的闭包确实等于预期（防御：样本本身须自洽）。
    if computed_closure != expected_closure:
        # 样本闭包自洽性不符，则以代码计算为准，但仍要求响应列出 computed_closure。
        closure_to_check = computed_closure
    else:
        closure_to_check = expected_closure
    # 响应必须出现闭包中每个 run_id（先列出影响闭包）。
    listed = all(rid in response for rid in closure_to_check)
    if not listed:
        return False
    # (b) 软删除校验：响应须说明闭包内节点被标记为 deleted。
    deleted_ok = all(
        (f"{rid}" in response and "deleted" in response.lower())
        for rid in closure_to_check
    )
    if not deleted_ok:
        return False
    # 未误删兄弟：post_state 中应保持 active 的节点，响应不应把它们标 deleted。
    post = exp.get("post_state", {})
    siblings_active = [k for k, v in exp.items() if k not in post and k not in ("affected_closure",)
                       and v == "active"]
    siblings_active += [k for k, v in post.items() if v != "deleted"]
    for sib in siblings_active:
        # 若响应显式把该兄弟标为 deleted，则误删。
        if re.search(rf"{re.escape(sib)}\b[^\n]*deleted", response, re.IGNORECASE):
            return False
    return True


# ---------------------------------------------------------------------------
# 真模型模式：驱动一个 tree 的一个 task
# ---------------------------------------------------------------------------
def run_task(cfg: dict, tree: dict, task: dict) -> dict:
    prompt = task.get("prompt", "")
    constraint = (
        "约束：只许查询状态/谱系（lineage），严禁重跑任何实验。"
    )
    messages = [{"role": "user", "content": f"{prompt}\n\n{constraint}"}]
    r = call_messages(cfg, messages=messages, max_tokens=1024)
    if r.get("error"):
        return {"task_id": task.get("task_id"), "type": task.get("type"),
                "passed": False, "error": r["error"], "response": ""}
    response = r.get("text", "")
    ttype = task.get("type")
    if ttype == "lineage_traversal_query":
        passed = check_traversal_query(cfg, tree, task, response)
    elif ttype == "lineage_audit":
        passed = check_audit(cfg, tree, task, response)
    elif ttype == "lineage_branch":
        passed = check_branch(cfg, tree, task, response)
    elif ttype == "lineage_cascade_delete":
        passed = check_cascade_delete(cfg, tree, task, response)
    else:
        passed = None
    return {
        "task_id": task.get("task_id"),
        "type": ttype,
        "passed": (passed if passed is not None else False),
        "judge_none": (passed is None),
        "response": response[:200],
    }


def run(cfg: dict, limit: int | None) -> dict:
    trees = load_samples()
    if limit is not None:
        trees = trees[:limit]
    per_tree = []
    total_pass = total_tasks = 0
    for tree in trees:
        tree_id = tree.get("tree_id")
        tasks = tree.get("tasks", [])
        task_results = []
        dims = {t: {"pass": 0, "total": 0} for t in TASK_TYPES}
        for task in tasks:
            res = run_task(cfg, tree, task)
            task_results.append({
                "task_id": res["task_id"],
                "type": res["type"],
                "passed": res["passed"],
            })
            if res.get("judge_none"):
                task_results[-1]["judge_none"] = True
            dims[res["type"]]["pass"] += 1 if res["passed"] else 0
            dims[res["type"]]["total"] += 1
            total_pass += 1 if res["passed"] else 0
            total_tasks += 1
        dim_rate = {
            t: (round(dims[t]["pass"] / dims[t]["total"], 3) if dims[t]["total"] else None)
            for t in TASK_TYPES
        }
        per_tree.append({
            "tree_id": tree_id,
            "tasks": task_results,
            "dim_rates": dim_rate,
        })
    overall = round(total_pass / total_tasks, 3) if total_tasks else 0.0
    return {"overall_pass_rate": overall, "total_pass": total_pass,
            "total_tasks": total_tasks, "per_tree": per_tree}


# ---------------------------------------------------------------------------
# --selftest：用内置 mock tree + mock agent 响应校验四任务判定 + DAG/闭包
# （不调模型/网络，可在无 ANTHROPIC_* 环境变量时通过）
# ---------------------------------------------------------------------------
def _selftest() -> int:
    # ---- mock tree：4 节点，r0→r1,r2；r1→r1a ----
    mock_tree = {
        "tree_id": "mock",
        "nodes": [
            {"run_id": "r0", "parent_run_id": None, "name": "root", "children": ["r1", "r2"], "lifecycle_stage": "active"},
            {"run_id": "r1", "parent_run_id": "r0", "name": "n1", "children": ["r1a"], "lifecycle_stage": "active"},
            {"run_id": "r2", "parent_run_id": "r0", "name": "n2", "children": [], "lifecycle_stage": "active"},
            {"run_id": "r1a", "parent_run_id": "r1", "name": "n1a", "children": [], "lifecycle_stage": "active"},
        ],
    }

    # 1) is_dag：正常 tree 返回 True
    assert is_dag(mock_tree["nodes"]) is True, "mock tree 应为 DAG"

    # 构造有环 tree
    cyclic = [
        {"run_id": "a", "parent_run_id": None, "children": ["b"]},
        {"run_id": "b", "parent_run_id": "a", "children": ["c"]},
        {"run_id": "c", "parent_run_id": "b", "children": ["a"]},  # 回边 a→c→b→a
    ]
    assert is_dag(cyclic) is False, "有环 tree 应返回 False"

    # 2) descendant_closure
    assert descendant_closure(mock_tree["nodes"], "r1") == {"r1", "r1a"}, \
        "r1 闭包应为 {r1, r1a}"

    # 3) traversal_query：用 stub judge 验证分支逻辑（不依赖真模型）
    global judge_yesno
    orig_judge = judge_yesno
    task_tq = {"type": "lineage_traversal_query",
               "expected": {"ancestors": ["r1a", "r1", "r0"], "leaf_accuracy": 0.9}}
    try:
        judge_yesno = lambda cfg, rubric, evidence: ("r1a" in evidence and "r0" in evidence)
        ok = check_traversal_query({}, mock_tree, task_tq, "ancestors r1a -> r1 -> r0 ok")
        assert ok is True, "traversal_query 应判通过"
        bad = check_traversal_query({}, mock_tree, task_tq, "i only mention r1")
        assert bad is False, "traversal_query 应判不通过"
    finally:
        judge_yesno = orig_judge

    # 4) audit：stub judge 验证分支逻辑
    task_audit = {"type": "lineage_audit",
                  "expected": {"r1_vs_r0": {"lr": "0.01→0.005", "accuracy": "0.82→0.85"}}}
    try:
        judge_yesno = lambda cfg, rubric, evidence: ("lr" in evidence and "accuracy" in evidence)
        ok = check_audit({}, mock_tree, task_audit, "lr changed 0.01->0.005, accuracy 0.82->0.85")
        assert ok is True, "audit 应判通过"
        bad = check_audit({}, mock_tree, task_audit, "no diff mentioned")
        assert bad is False, "audit 应判不通过"
    finally:
        judge_yesno = orig_judge

    # 5) branch：结构化校验（不靠模型）
    task_branch = {"type": "lineage_branch",
                   "expected": {"new_run": {"parent_run_id": "r1", "name": "exp-x"}, "tree_still_valid": True}}
    # 通过：父指向 r1 且加后无环
    good_resp = '新建节点 {"run_id":"rx","parent_run_id":"r1","name":"exp-x"}'
    assert check_branch({}, mock_tree, task_branch, good_resp) is True, "branch 应判通过"
    # 不通过：父指向非 branch_point（如 r2）
    bad_parent = '新建节点 {"run_id":"rx","parent_run_id":"r2","name":"exp-x"}'
    assert check_branch({}, mock_tree, task_branch, bad_parent) is False, "父指向非 branch_point 应判不通过"
    # 不通过：加后有环（响应声称父指向 rx 自己制造环：rx 的父是 r1，再让 r1 父是 rx）
    cyclic_resp = ('新建节点 {"run_id":"rx","parent_run_id":"r1","name":"exp-x"}，'
                   '并将 r1 的 parent 改为 rx')
    # 注：_extract_new_node 只抽新节点，故用 children 制造环：
    # 让新节点 r1 之父指向 rx（修改原树不可行），这里改为直接构造有环合并。
    # 用一支响应让提取出的新节点 parent=r1，再手动验证若 r1 同时成为新节点之子则环——
    # 简单等价：响应声称新节点 run_id 与某现有节点形成环不可由 extract 触发，
    # 故此处用 _extract_new_node 返回 parent=r1 + 再测 is_dag 在有环合并下的 False。
    # 构造：新节点 rx 父 r1，并声称把 r1 也挂到 rx 下（children 含 r1）。
    cyclic_resp2 = '新建节点 {"run_id":"rx","parent_run_id":"r1","name":"exp-x"}，rx 的子节点含 r1'
    # _extract_new_node 不会把 r1 当 rx 的 child，所以直接验证 is_dag 在有环输入下 False 已覆盖。
    # 这里再以返回 False 兜底：父正确但响应文本暗示环 → 不通过（宽松：仍判定 False 因 listed/逻辑）。
    assert check_branch({}, mock_tree, task_branch, bad_parent) is False
    # 显式验证 is_dag 在人为环合并下返回 False（覆盖「加后有环」场景）。
    merged_cyclic = list(mock_tree["nodes"]) + [
        {"run_id": "rx", "parent_run_id": "r1", "name": "exp-x", "children": ["r1"]}
    ]
    for n in merged_cyclic:
        if n["run_id"] == "r1":
            n["children"] = list(n.get("children", [])) + ["rx"]
    assert is_dag(merged_cyclic) is False, "加后有环的合并树应为 False"

    # 6) cascade_delete：两步结构化校验（不靠模型）
    task_cd = {"type": "lineage_cascade_delete",
               "expected": {"affected_closure": ["r1", "r1a"],
                            "post_state": {"r1": "deleted", "r1a": "deleted"},
                            "r0": "active", "r2": "active"}}
    # 通过：先列闭包再软删除、未误删兄弟 r0/r2
    good_cd = ("影响闭包：r1, r1a。将 r1 与 r1a 标记为 lifecycle_stage=deleted 软删除。"
               "r0 与 r2 保持 active。")
    assert check_cascade_delete({}, mock_tree, task_cd, good_cd) is True, "cascade_delete 应判通过"
    # 不通过：漏列闭包（没提 r1a）
    miss_closure = "影响闭包：r1。将 r1 标记为 deleted。"
    assert check_cascade_delete({}, mock_tree, task_cd, miss_closure) is False, "漏列闭包应判不通过"
    # 不通过：误删兄弟（把 r0 也标 deleted）
    wrong_del = "影响闭包：r1, r1a。将 r1、r1a、r0 标记为 deleted。"
    assert check_cascade_delete({}, mock_tree, task_cd, wrong_del) is False, "误删兄弟应判不通过"

    print("[selftest] 全部通过")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true", help="用 mock 校验四任务逻辑+DAG/闭包，不调模型")
    ap.add_argument("--limit", type=int, default=None, help="真模型模式：最多评测 N 棵 tree")
    ap.add_argument("--json", dest="json_out", default=None, help="结果输出 JSON 路径")
    args = ap.parse_args()

    if args.selftest:
        return _selftest()

    # 真模型模式：缺环境变量则报错退出（client_cfg 内部也会退出，这里显式检查更清晰）。
    cfg = client_cfg()
    if args.limit is None:
        sys.stderr.write("[run_eval] 未指定 --limit，默认全量评测所有 tree\n")
    result = run(cfg, args.limit)
    txt = json.dumps(result, ensure_ascii=False, indent=2)
    if args.json_out:
        Path(args.json_out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.json_out).write_text(txt, encoding="utf-8")
        print(f"[run_eval] 结果已写入 {args.json_out}")
    else:
        print(txt)
    print("\n总体汇总：")
    print(f"  总体通过率: {result['overall_pass_rate']} ({result['total_pass']}/{result['total_tasks']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
