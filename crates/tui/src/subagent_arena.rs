//! Arena 多模型对战 与 Team 领导角色骨架。
//!
//! 这是 mimofan multi-agent 协同层的两个新增原语，位于已有的
//! [`crate::tools::subagent`]（SubAgentManager / SpawnRequest / task_graph
//! 等）之上，但**不修改任何现有 subagent 业务代码**。
//!
//! ## 语义边界
//!
//! - **Arena（多模型对战）**：给定一个 *相同的 prompt*，并行启动多个
//!   `(模型 / provider / 配置)` 组合的子 agent，收集各自输出后做结构化对比。
//!   Arena 关心的是「同一个问题，不同选手的表现差异」，选手之间**互不通信**。
//!   它是对 `spawn_subagent_from_input` 的「同 prompt 多配置扇出」封装。
//!
//! - **Team（领导角色）**：一个 `Leader` 接收一个大任务，先做**任务分解**
//!   （复用 [`crate::tools::subagent::decomposer::TaskDecomposer`] 的能力），
//!   再把子任务分派给多个子 agent（可并行），最后**汇总**结果。
//!   Leader 只做协调、不做实际生成 —— 它与 Arena 的关键区别是：子任务 prompt
//!   各不相同，且存在「先分解、后分派、再汇总」的明确生命周期。
//!
//! ## 为什么用 `AgentRunner` trait 而不是直接调用 `spawn_subagent_from_input`
//!
//! 真实 spawn 需要一个联网的 [`crate::client::ApiClient`]，会让单元测试必须
//! 依赖外部模型/provider。这里抽出一个极薄的 [`AgentRunner`] trait：
//!
//! - 生产路径上用 [`SpawnSubagentRunner`] 把调用转发到真实的
//!   `spawn_subagent_from_input`，保持与现有 SubAgentManager 的 API 风格一致；
//! - 测试路径上用 [`FakeAgentRunner`]（内存 fake），无需任何网络即可验证
//!   「并行收集 + compare 结构」「分解-分派-汇总流程」。
//!
//! 这样 Arena / Team 的核心调度逻辑与具体执行后端解耦，既能被现有 spawn
//! 链路复用，也能在零网络依赖下被完整测试。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::tools::subagent::decomposer::{TaskDecomposer, TaskGraph};
use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentResult, SubAgentRuntime, SubAgentType,
};

/// 一个对战/分派「选手」的配置描述。Arena 与 Team 都用它来表达「派谁去做」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestantConfig {
    /// 选手唯一标识（Arena 中用于区分不同模型；Team 中用于区分不同子 worker）。
    pub id: String,
    /// 人类可读名称，默认回退到 `id`。
    pub name: Option<String>,
    /// 模型名（如 `deepseek-chat` / `miMo`）。具体含义取决于 runner 后端。
    pub model: String,
    /// provider 名（如 `deepseek` / `xiaomi`）。
    pub provider: String,
    /// 可选的子 agent 角色类型；Arena 对战通常不指定（同 prompt 同角色），
    /// Team 分派时按子任务语义指定。
    #[serde(default)]
    pub agent_type: SubAgentType,
    /// 透传给 runner 的额外配置（温度、max_tokens 等）。不强制 schema，
    /// 由具体 `AgentRunner` 解释，保证 Arena/Team 不耦合任何单 provider 参数。
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

impl ContestantConfig {
    /// 构造一个最简对战配置：id + model + provider。
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: None,
            model: model.into(),
            provider: provider.into(),
            agent_type: SubAgentType::General,
            extra: HashMap::new(),
        }
    }

    /// 友好显示名：优先用 `name`，否则回退到 `id`。
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// 单个选手在 Arena / Team 中的一次运行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestantOutcome {
    /// 对应 [`ContestantConfig::id`]。
    pub contestant_id: String,
    /// 选手展示名（冗余存储，便于直接渲染对比表）。
    pub name: String,
    /// 使用的 model / provider 快照（来自配置）。
    pub model: String,
    pub provider: String,
    /// 最终文本输出。
    pub output: String,
    /// 墙钟耗时（毫秒）。
    pub duration_ms: u64,
    /// 该选手此次运行的 token 估算（由 runner 回报；fake 用占位值）。
    #[serde(default)]
    pub input_tokens: u64,
    /// 该选手此次运行的输出 token 估算。
    #[serde(default)]
    pub output_tokens: u64,
    /// 是否失败。失败时 `output` 承载错误信息。
    #[serde(default)]
    pub failed: bool,
}

/// 抽象「派一个子 agent 去跑一个 prompt」的执行后端。
///
/// 设计理由：Arena/Team 的调度逻辑（并行扇出、收集、汇总）与「如何真正启动
/// 一个子 agent」解耦。`spawn_subagent_from_input` 需要联网 client，无法用于
/// 无网络单测，因此用此 trait 注入后端。
#[async_trait]
pub trait AgentRunner: Send + Sync {
    /// 用给定配置运行 `prompt`，返回结构化结果。
    ///
    /// `manager` / `runtime` 仅供生产 runner 转发给真实 spawn 链路使用；
    /// 测试 runner 可忽略。
    async fn run(
        &self,
        config: &ContestantConfig,
        prompt: &str,
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
    ) -> Result<ContestantOutcome, anyhow::Error>;
}

/// 生产用 runner：把调用转发到现有的 `spawn_subagent_from_input`。
///
/// 保持与 [`crate::tools::subagent`] 现有 API 风格一致——Arena/Team 不重复造
/// spawn 轮子，而是复用既有 manager + runtime。
#[derive(Clone)]
pub struct SpawnSubagentRunner;

#[async_trait]
impl AgentRunner for SpawnSubagentRunner {
    async fn run(
        &self,
        config: &ContestantConfig,
        prompt: &str,
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
    ) -> Result<ContestantOutcome, anyhow::Error> {
        // 复用现有 spawn 入参解析 + 执行路径。这里把 ContestantConfig 翻译成
        // 既有的 spawn request JSON，避免 Arena/Team 直接依赖内部 SpawnRequest
        // 字段（那些字段是 pub(crate) 且易变）。
        let input = serde_json::json!({
            "prompt": prompt,
            "agent_type": config.agent_type.as_str(),
            "model": config.model,
            "provider": config.provider,
            "nickname": config.display_name(),
        });
        let started = Instant::now();
        let result: SubAgentResult =
            crate::tools::subagent::tool::spawn_subagent_from_input(input, manager, runtime)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let failed = matches!(result.status, SubAgentStatusAlias::Failed(_));
        Ok(ContestantOutcome {
            contestant_id: config.id.clone(),
            name: config.display_name().to_string(),
            model: config.model.clone(),
            provider: config.provider.clone(),
            output: result.result.unwrap_or_default(),
            duration_ms,
            input_tokens: 0,
            output_tokens: 0,
            failed,
        })
    }
}

// 仅用于上面的失败判断，避免直接 import 大枚举路径的别名。
use crate::tools::subagent::SubAgentStatus as SubAgentStatusAlias;

/// 内存 fake runner：用于单元测试，不依赖真实网络/模型。
///
/// 行为可配置：通过 `responses` 按 `contestant_id` 映射「固定返回文本」；
/// 命中则原样返回（并带一个确定性 token 估算）；未命中则构造一个
/// 「{name} on {model}/{provider}: <prompt 前 32 字符>」的占位结果，
/// 这样测试可以断言 Arena 的确把 *相同 prompt* 派给了不同选手。
#[derive(Clone)]
pub struct FakeAgentRunner {
    responses: Arc<RwLock<HashMap<String, String>>>,
    fail_ids: Arc<RwLock<Vec<String>>>,
}

impl Default for FakeAgentRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAgentRunner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            responses: Arc::new(RwLock::new(HashMap::new())),
            fail_ids: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 为某个 contestant_id 设定固定返回文本（测试用）。
    pub async fn set_response(&self, contestant_id: impl Into<String>, text: impl Into<String>) {
        self.responses
            .write()
            .await
            .insert(contestant_id.into(), text.into());
    }

    /// 标记某个 contestant_id 必定失败（测试失败路径）。
    pub async fn set_failure(&self, contestant_id: impl Into<String>) {
        self.fail_ids.write().await.push(contestant_id.into());
    }
}

#[async_trait]
impl AgentRunner for FakeAgentRunner {
    async fn run(
        &self,
        config: &ContestantConfig,
        prompt: &str,
        _manager: SharedSubAgentManager,
        _runtime: SubAgentRuntime,
    ) -> Result<ContestantOutcome, anyhow::Error> {
        let started = Instant::now();
        // 人为小延迟以让「并行调度」在测试中可观测（futures 仍并发执行）。
        tokio::time::sleep(Duration::from_millis(2)).await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        let fail_ids = self.fail_ids.read().await;
        if fail_ids.iter().any(|id| id == &config.id) {
            return Err(anyhow::anyhow!(
                "fake runner forced failure for {}",
                config.id
            ));
        }

        let responses = self.responses.read().await;
        let output = match responses.get(&config.id) {
            Some(text) => text.clone(),
            None => format!(
                "{} on {}/{}: {}",
                config.display_name(),
                config.provider,
                config.model,
                prompt.chars().take(32).collect::<String>()
            ),
        };
        // 确定性 token 估算：按字符数，便于测试断言 compare 结构。
        let output_tokens = output.chars().count() as u64;
        Ok(ContestantOutcome {
            contestant_id: config.id.clone(),
            name: config.display_name().to_string(),
            model: config.model.clone(),
            provider: config.provider.clone(),
            output,
            duration_ms,
            input_tokens: prompt.chars().count() as u64,
            output_tokens,
            failed: false,
        })
    }
}

/// Arena：同一 prompt 并行跑多个模型/配置，收集并对比输出。
#[derive(Clone)]
pub struct Arena {
    runner: Arc<dyn AgentRunner>,
}

impl Arena {
    /// 用指定执行后端构造一个 Arena。
    #[must_use]
    pub fn new(runner: Arc<dyn AgentRunner>) -> Self {
        Self { runner }
    }

    /// 用生产 runner（真实 spawn 链路）构造 Arena。
    #[must_use]
    pub fn with_spawn_runner() -> Self {
        Self::new(Arc::new(SpawnSubagentRunner))
    }

    /// 用内存 fake runner 构造 Arena（测试用）。
    #[must_use]
    pub fn with_fake_runner(fake: FakeAgentRunner) -> Self {
        Self::new(Arc::new(fake))
    }

    /// 给定同一 prompt 和多个对战配置，并行 spawn 子 agent 并等待全部完成。
    ///
    /// 使用 `FuturesUnordered` 并发驱动，不保证返回顺序即发起顺序——调用方
    /// 应依赖 `contestant_id` 区分结果。任一对战失败会作为
    /// [`ArenaOutcome::errors`] 记录，而**不**让整个 Arena 失败（对战的价值
    /// 正是「看谁挂了、谁活着」）。
    pub async fn run(
        &self,
        prompt: &str,
        contestants: &[ContestantConfig],
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
    ) -> ArenaOutcome {
        let started = Instant::now();
        let mut futures = futures_util::stream::FuturesUnordered::new();
        for cfg in contestants {
            let runner = self.runner.clone();
            let cfg = cfg.clone();
            let prompt = prompt.to_string();
            let manager = manager.clone();
            let runtime = runtime.clone();
            futures.push(async move {
                match runner.run(&cfg, &prompt, manager, runtime).await {
                    Ok(outcome) => Ok(outcome),
                    Err(err) => Err((cfg.id.clone(), err.to_string())),
                }
            });
        }

        let mut outcomes = Vec::new();
        let mut errors = Vec::new();
        while let Some(item) = futures.next().await {
            match item {
                Ok(outcome) => outcomes.push(outcome),
                Err((id, msg)) => errors.push((id, msg)),
            }
        }

        let total_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        ArenaOutcome {
            prompt: prompt.to_string(),
            outcomes,
            errors,
            total_ms,
        }
    }
}

/// Arena 一次对战的整体结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaOutcome {
    /// 本场对战使用的统一 prompt。
    pub prompt: String,
    /// 各选手成功输出（按完成顺序，非发起顺序）。
    pub outcomes: Vec<ContestantOutcome>,
    /// `(contestant_id, error_msg)`：失败选手列表。
    pub errors: Vec<(String, String)>,
    /// 整场墙钟耗时（毫秒）。
    pub total_ms: u64,
}

impl ArenaOutcome {
    /// 结构化对比：把每场对战的输出、耗时、token、成败平铺为可读报告。
    ///
    /// 设计为「纯函数、无副作用、可序列化」——便于上层直接喂给模型或落盘。
    /// 对比维度固定为：output 文本、duration_ms、input/output token、failed。
    #[must_use]
    pub fn compare(&self) -> ArenaComparison {
        let fastest = self
            .outcomes
            .iter()
            .min_by_key(|o| o.duration_ms)
            .map(|o| o.contestant_id.clone());
        let most_tokens = self
            .outcomes
            .iter()
            .max_by_key(|o| o.output_tokens)
            .map(|o| o.contestant_id.clone());
        ArenaComparison {
            prompt: self.prompt.clone(),
            total_ms: self.total_ms,
            contestant_count: self.outcomes.len() + self.errors.len(),
            succeeded: self.outcomes.len(),
            failed: self.errors.len(),
            fastest_contestant: fastest,
            most_verbose_contestant: most_tokens,
            rows: self
                .outcomes
                .iter()
                .map(|o| ArenaCompareRow {
                    contestant_id: o.contestant_id.clone(),
                    name: o.name.clone(),
                    model: o.model.clone(),
                    provider: o.provider.clone(),
                    duration_ms: o.duration_ms,
                    input_tokens: o.input_tokens,
                    output_tokens: o.output_tokens,
                    failed: o.failed,
                    output_excerpt: o.output.chars().take(200).collect(),
                })
                .collect(),
            errors: self.errors.clone(),
        }
    }
}

/// `compare()` 产出的结构化对比视图。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaComparison {
    pub prompt: String,
    pub total_ms: u64,
    pub contestant_count: usize,
    pub succeeded: usize,
    pub failed: usize,
    /// 耗时最短的选手 id（无成功者时为 None）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fastest_contestant: Option<String>,
    /// 输出 token 最多的选手 id（无成功者时为 None）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most_verbose_contestant: Option<String>,
    pub rows: Vec<ArenaCompareRow>,
    pub errors: Vec<(String, String)>,
}

/// 单行对比（每个选手一行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaCompareRow {
    pub contestant_id: String,
    pub name: String,
    pub model: String,
    pub provider: String,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub failed: bool,
    pub output_excerpt: String,
}

// ===========================================================================
// Team（领导角色）
// ===========================================================================

/// Leader 分解出的一个子任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    /// 子任务唯一 id。
    pub id: String,
    /// 该子任务要派给哪个 contestant（对应 [`ContestantConfig::id`]）。
    pub assignee: String,
    /// 该子任务的具体 prompt（由 Leader 基于大任务 + 分解产生，各不相同）。
    pub prompt: String,
    /// 可选依赖：仅当这些 task id 全部成功后才分派本任务。
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 子任务语义角色。
    #[serde(default)]
    pub agent_type: SubAgentType,
}

/// Team 的 Leader 角色：只做「分解 → 分派 → 汇总」，不亲自生成内容。
#[derive(Clone)]
pub struct TeamLeader {
    runner: Arc<dyn AgentRunner>,
    /// 分解器：复用现有 task_graph 的分解能力。
    decomposer: TaskDecomposer,
}

impl TeamLeader {
    /// 用指定执行后端与分解器构造 Leader。
    #[must_use]
    pub fn new(runner: Arc<dyn AgentRunner>) -> Self {
        Self {
            runner,
            decomposer: TaskDecomposer,
        }
    }

    /// 用 fake runner 构造 Leader（测试用）。
    #[must_use]
    pub fn with_fake_runner(fake: FakeAgentRunner) -> Self {
        Self::new(Arc::new(fake))
    }

    /// 用生产 spawn runner 构造 Leader。
    #[must_use]
    pub fn with_spawn_runner() -> Self {
        Self::new(Arc::new(SpawnSubagentRunner))
    }

    /// **分解**：把一个大任务拆成若干子任务。
    ///
    /// 这里不重新发明分解算法，而是委托给 [`TaskDecomposer`]（与 `run_task_graph`
    /// 同源）。默认按「语义子目标」切分；真实部署可由模型调用产生更细的图。
    /// 为保持骨架可测、无网络依赖，本方法返回一个 *确定性* 的分解骨架：
    /// 若 `tasks` 已由调用方提供则原样采用，否则按 `subtasks` 文案生成若干
    /// 串行/并行的 `TeamTask`。
    #[must_use]
    pub fn decompose(&self, objective: &str, subtasks: &[String]) -> Vec<TeamTask> {
        if subtasks.is_empty() {
            // 无显式子任务时，退化为「单一整体子任务」交给默认 worker。
            return vec![TeamTask {
                id: "task-0".to_string(),
                assignee: "default".to_string(),
                prompt: objective.to_string(),
                depends_on: Vec::new(),
                agent_type: SubAgentType::General,
            }];
        }
        subtasks
            .iter()
            .enumerate()
            .map(|(i, sub)| TeamTask {
                id: format!("task-{i}"),
                assignee: "default".to_string(),
                prompt: format!("{objective}\n\n子目标 {i}: {sub}"),
                depends_on: Vec::new(),
                agent_type: SubAgentType::General,
            })
            .collect()
    }

    /// 把子任务指派给对应 contestant 并运行（支持 `depends_on` 顺序约束）。
    ///
    /// 调度策略：维护「已完成 id 集合」，每轮把所有「依赖已满足且未运行」的
    /// 任务并行 spawn；任一下游任务的前置失败时，该任务标记为
    /// [`TeamTaskStatus::Skipped`]，与 task_graph 的失败传播一致。
    pub async fn dispatch(
        &self,
        tasks: &[TeamTask],
        contestants: &HashMap<String, ContestantConfig>,
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
    ) -> TeamOutcome {
        let started = Instant::now();
        let mut results: HashMap<String, TeamTaskResult> = HashMap::new();
        let mut done: HashMap<String, bool> = HashMap::new();

        let total = tasks.len();
        let mut remaining: Vec<TeamTask> = tasks.to_vec();

        loop {
            // 选出本「波次」可运行任务：依赖全部 done 成功。
            let batch: Vec<TeamTask> = remaining
                .iter()
                .filter(|t| {
                    t.depends_on.iter().all(|d| {
                        matches!(results.get(d), Some(r) if r.status == TeamTaskStatus::Completed)
                    })
                })
                .cloned()
                .collect();
            if batch.is_empty() {
                break; // 剩下的都是因前置失败而跳过的，或已无任务。
            }

            // 从 remaining 中移除本波次任务。
            remaining.retain(|t| !batch.iter().any(|b| b.id == t.id));

            let mut futures = futures_util::stream::FuturesUnordered::new();
            for task in &batch {
                let cfg = contestants.get(&task.assignee).cloned().unwrap_or_else(|| {
                    // 没有显式 contestant 时，用默认配置兜底（保证骨架不崩）。
                    ContestantConfig::new(
                        task.assignee.clone(),
                        "default".to_string(),
                        "default".to_string(),
                    )
                });
                let runner = self.runner.clone();
                let cfg = cfg.clone();
                let prompt = task.prompt.clone();
                let manager = manager.clone();
                let runtime = runtime.clone();
                let task_id = task.id.clone();
                futures.push(async move {
                    match runner.run(&cfg, &prompt, manager, runtime).await {
                        Ok(outcome) => (task_id, Ok(outcome)),
                        Err(err) => (task_id, Err(err.to_string())),
                    }
                });
            }

            while let Some((task_id, res)) = futures.next().await {
                match res {
                    Ok(outcome) => {
                        done.insert(task_id.clone(), true);
                        results.insert(
                            task_id,
                            TeamTaskResult {
                                task_id: outcome.contestant_id.clone(),
                                assignee: outcome.name.clone(),
                                status: TeamTaskStatus::Completed,
                                output: outcome.output,
                                error: None,
                            },
                        );
                    }
                    Err(msg) => {
                        let task_id_key = task_id.clone();
                        let assignee_name = contestants
                            .get(&task_id)
                            .map(|c| c.display_name().to_string())
                            .unwrap_or_else(|| task_id.clone());
                        done.insert(task_id_key.clone(), false);
                        results.insert(
                            task_id_key,
                            TeamTaskResult {
                                task_id: task_id.clone(),
                                assignee: assignee_name,
                                status: TeamTaskStatus::Failed,
                                output: String::new(),
                                error: Some(msg),
                            },
                        );
                    }
                }
            }
        }

        // 残余未运行任务 = 因前置失败被跳过。
        for task in &remaining {
            results.insert(
                task.id.clone(),
                TeamTaskResult {
                    task_id: task.id.clone(),
                    assignee: contestants
                        .get(&task.assignee)
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_else(|| task.assignee.clone()),
                    status: TeamTaskStatus::Skipped,
                    output: String::new(),
                    error: Some("skipped: upstream dependency failed".to_string()),
                },
            );
        }

        let total_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        TeamOutcome {
            total_tasks: total,
            total_ms,
            results: results.into_values().collect(),
        }
    }

    /// 端到端：**分解 → 分派 → 汇总**。
    ///
    /// `objective` 是大任务文案，`subtasks` 是调用方（或模型）给出的子目标列表，
    /// `contestants` 是可用 worker 配置表。返回汇总结果。
    pub async fn execute(
        &self,
        objective: &str,
        subtasks: &[String],
        contestants: &HashMap<String, ContestantConfig>,
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
    ) -> TeamOutcome {
        let tasks = self.decompose(objective, subtasks);
        let outcome = self.dispatch(&tasks, contestants, manager, runtime).await;
        // 汇总：在真实部署里 Leader 会基于 `outcome.results` 再生成一段总结；
        // 骨架阶段「汇总」即返回结构化结果集，供上层模型聚合。
        outcome
    }
}

/// Team 子任务运行状态（与 `task_graph` 的 `TaskNodeStatus` 对齐语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    Completed,
    Failed,
    Skipped,
}

/// Team 单个子任务的运行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTaskResult {
    pub task_id: String,
    pub assignee: String,
    pub status: TeamTaskStatus,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Team 一次领导调度的整体结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamOutcome {
    pub total_tasks: usize,
    pub total_ms: u64,
    pub results: Vec<TeamTaskResult>,
}

impl TeamOutcome {
    /// 汇总统计：完成 / 失败 / 跳过的计数，供 Leader 决策或上报。
    #[must_use]
    pub fn summary(&self) -> TeamSummary {
        let mut completed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        for r in &self.results {
            match r.status {
                TeamTaskStatus::Completed => completed += 1,
                TeamTaskStatus::Failed => failed += 1,
                TeamTaskStatus::Skipped => skipped += 1,
            }
        }
        TeamSummary {
            total_tasks: self.total_tasks,
            completed,
            failed,
            skipped,
            total_ms: self.total_ms,
        }
    }
}

/// Team 汇总统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSummary {
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_ms: u64,
}

// 让 `TaskGraph` 在 Team 模块内成为可达的既有类型别名引用（避免未使用告警）。
#[allow(dead_code)]
fn _assert_task_graph_reachable(_g: &TaskGraph) {}

/// 生成一个唯一的 Arena/Team 运行 id（测试与日志用）。
#[must_use]
pub fn new_run_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ApiClient;
    use crate::config::Config;
    use crate::tools::spec::ToolContext;
    use crate::tools::subagent::SubAgentManager;
    use std::sync::Arc as StdArc;

    /// 测试用 manager：走与 adversarial 测试相同的「最小可构造」路径。
    /// Arena/Team 在 fake runner 下不触碰其内部状态，仅占位以满足类型。
    fn fake_manager() -> SharedSubAgentManager {
        StdArc::new(RwLock::new(SubAgentManager::new(std::env::temp_dir(), 1)))
    }

    /// 测试用 runtime：项目已有 detached client + 临时 ToolContext 构造方式。
    fn fake_runtime() -> SubAgentRuntime {
        SubAgentRuntime::new(
            ApiClient::new_detached(&Config::default()).expect("test client"),
            "test-model".to_string(),
            ToolContext::new(std::env::temp_dir()),
            false,
            None,
            fake_manager(),
        )
    }

    // ---- Arena 测试 ----

    #[tokio::test]
    async fn arena_collects_all_contestants_in_parallel() {
        let fake = FakeAgentRunner::new();
        fake.set_response("m1", "answer from model one").await;
        fake.set_response("m2", "answer from model two").await;

        let arena = Arena::with_fake_runner(fake);
        let contestants = vec![
            ContestantConfig::new("m1", "deepseek-chat", "deepseek"),
            ContestantConfig::new("m2", "miMo", "xiaomi"),
        ];
        let outcome = arena
            .run("What is 2+2?", &contestants, fake_manager(), fake_runtime())
            .await;

        assert_eq!(outcome.outcomes.len(), 2, "两个选手都应成功收集");
        assert!(outcome.errors.is_empty(), "无失败");
        let ids: Vec<&str> = outcome
            .outcomes
            .iter()
            .map(|o| o.contestant_id.as_str())
            .collect();
        assert!(ids.contains(&"m1"));
        assert!(ids.contains(&"m2"));
    }

    #[tokio::test]
    async fn arena_compare_structure_has_metrics() {
        let fake = FakeAgentRunner::new();
        fake.set_response("a", "short").await;
        fake.set_response("b", "a much longer answer with many tokens inside it for b")
            .await;

        let arena = Arena::with_fake_runner(fake);
        let contestants = vec![
            ContestantConfig::new("a", "m1", "p1"),
            ContestantConfig::new("b", "m2", "p2"),
        ];
        let outcome = arena
            .run("prompt-x", &contestants, fake_manager(), fake_runtime())
            .await;
        let cmp = outcome.compare();

        assert_eq!(cmp.contestant_count, 2);
        assert_eq!(cmp.succeeded, 2);
        assert_eq!(cmp.failed, 0);
        assert!(cmp.rows.iter().any(|r| r.contestant_id == "a"));
        assert!(cmp.rows.iter().any(|r| r.contestant_id == "b"));
        // most_verbose 应为输出更长的 b
        assert_eq!(cmp.most_verbose_contestant.as_deref(), Some("b"));
        // fastest 应存在（两个都成功）
        assert!(cmp.fastest_contestant.is_some());
    }

    #[tokio::test]
    async fn arena_records_failures_without_poisoning() {
        let fake = FakeAgentRunner::new();
        fake.set_response("ok", "fine").await;
        fake.set_failure("bad").await;

        let arena = Arena::with_fake_runner(fake);
        let contestants = vec![
            ContestantConfig::new("ok", "m1", "p1"),
            ContestantConfig::new("bad", "m2", "p2"),
        ];
        let outcome = arena
            .run("p", &contestants, fake_manager(), fake_runtime())
            .await;

        assert_eq!(outcome.outcomes.len(), 1);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(outcome.errors[0].0, "bad");
        let cmp = outcome.compare();
        assert_eq!(cmp.failed, 1);
        assert_eq!(cmp.succeeded, 1);
    }

    #[tokio::test]
    async fn arena_same_prompt_reaches_all_contestants() {
        let fake = FakeAgentRunner::new();
        // 不设定固定返回，fake 会在输出里回显 prompt 前 32 字符。
        let arena = Arena::with_fake_runner(fake);
        let contestants = vec![
            ContestantConfig::new("x", "mx", "px"),
            ContestantConfig::new("y", "my", "py"),
        ];
        let prompt = "UNIQUE-PROMPT-FOR-ARENA-TEST-12345";
        let outcome = arena
            .run(prompt, &contestants, fake_manager(), fake_runtime())
            .await;
        for o in &outcome.outcomes {
            assert!(
                o.output.contains("UNIQUE-PROMPT-FOR-ARENA-TEST"),
                "相同 prompt 必须派到每个选手"
            );
        }
    }

    // ---- Team 测试 ----

    #[tokio::test]
    async fn team_decompose_then_dispatch_then_summarize() {
        let fake = FakeAgentRunner::new();
        fake.set_response("default", "subtask done").await;

        let leader = TeamLeader::with_fake_runner(fake);
        let objective = "Build a feature";
        let subtasks = vec![
            "design".to_string(),
            "implement".to_string(),
            "test".to_string(),
        ];

        let tasks = leader.decompose(objective, &subtasks);
        assert_eq!(tasks.len(), 3, "应分解为 3 个子任务");
        assert!(tasks.iter().all(|t| t.depends_on.is_empty()));

        let mut contestants = HashMap::new();
        contestants.insert(
            "default".to_string(),
            ContestantConfig::new("default", "m", "p"),
        );

        let outcome = leader
            .dispatch(&tasks, &contestants, fake_manager(), fake_runtime())
            .await;
        let summary = outcome.summary();

        assert_eq!(summary.total_tasks, 3);
        assert_eq!(summary.completed, 3);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
        assert!(
            outcome
                .results
                .iter()
                .all(|r| r.status == TeamTaskStatus::Completed)
        );
    }

    #[tokio::test]
    async fn team_execute_end_to_end() {
        let fake = FakeAgentRunner::new();
        fake.set_response("default", "ok").await;

        let leader = TeamLeader::with_fake_runner(fake);
        let mut contestants = HashMap::new();
        contestants.insert(
            "default".to_string(),
            ContestantConfig::new("default", "m", "p"),
        );

        let outcome = leader
            .execute(
                "Big job",
                &["step A".to_string(), "step B".to_string()],
                &contestants,
                fake_manager(),
                fake_runtime(),
            )
            .await;
        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.summary().completed, 2);
    }

    #[tokio::test]
    async fn team_skips_downstream_on_failure() {
        let fake = FakeAgentRunner::new();
        // 让 task-0 失败，task-1 依赖它 → 应被跳过。
        fake.set_failure("default").await;

        let leader = TeamLeader::with_fake_runner(fake);
        // 构造带依赖的任务：task-1 depends_on task-0
        let tasks = vec![
            TeamTask {
                id: "task-0".to_string(),
                assignee: "default".to_string(),
                prompt: "root".to_string(),
                depends_on: vec![],
                agent_type: SubAgentType::General,
            },
            TeamTask {
                id: "task-1".to_string(),
                assignee: "default".to_string(),
                prompt: "child".to_string(),
                depends_on: vec!["task-0".to_string()],
                agent_type: SubAgentType::General,
            },
        ];
        let mut contestants = HashMap::new();
        contestants.insert(
            "default".to_string(),
            ContestantConfig::new("default", "m", "p"),
        );

        let outcome = leader
            .dispatch(&tasks, &contestants, fake_manager(), fake_runtime())
            .await;
        let summary = outcome.summary();
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.completed, 0);
    }
}
