//! Tool registry for managing and executing tools.
//!
//! The registry provides:
//! - Dynamic tool registration
//! - Tool lookup by name
//! - Conversion to API Tool format
//! - Filtering by capability

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use std::path::{Path, PathBuf};

use mimofan_protocol::runtime::DynamicToolSpec;
use serde_json::Value;

use crate::client::ApiClient;
use crate::models::Tool;

use super::schema_canonicalize;
use super::schema_sanitize;
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

// === Types ===

/// Registry that holds all available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolSpec>>,
    context: ToolContext,
    /// Memoised serialised tool catalog. Rebuilt lazily on first
    /// `to_api_tools` call after a mutation; pinned across reads so the
    /// description and schema bytes stay byte-stable for DeepSeek's KV
    /// prefix cache. Invalidated on `register` / `remove` / `clear`.
    api_cache: OnceLock<Vec<Tool>>,
}

impl ToolRegistry {
    /// Create a new empty registry with the given context.
    #[must_use]
    pub fn new(context: ToolContext) -> Self {
        Self {
            tools: HashMap::new(),
            context,
            api_cache: OnceLock::new(),
        }
    }

    /// Register a tool in the registry.
    pub fn register(&mut self, tool: Arc<dyn ToolSpec>) {
        let name = tool.name().to_string();
        if self.tools.insert(name.clone(), tool).is_some() {
            tracing::warn!("Overwriting existing tool: {}", name);
        }
        self.invalidate_api_cache();
    }

    /// Register multiple tools at once.
    pub fn register_all(&mut self, tools: Vec<Arc<dyn ToolSpec>>) {
        for tool in tools {
            self.register(tool);
        }
    }

    /// Get a tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolSpec>> {
        self.tools.get(name).cloned()
    }

    /// Check if a tool exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get all registered tool names.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(std::string::String::as_str).collect()
    }

    /// Get the number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Get all registered tools.
    #[must_use]
    pub fn all(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools.values().cloned().collect()
    }

    /// Execute a tool by name with the given input.
    pub async fn execute(&self, name: &str, input: Value) -> Result<String, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::not_available(format!("tool '{name}' is not registered")))?;

        let result = tool.execute(input, &self.context).await?;
        Ok(result.content)
    }

    /// Execute a tool by name, returning the full `ToolResult`.
    pub async fn execute_full(&self, name: &str, input: Value) -> Result<ToolResult, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::not_available(format!("tool '{name}' is not registered")))?;

        tracing::debug!(
            trace_id = %self.context.trace_id,
            tool = name,
            "dispatching tool execution"
        );
        tool.execute(input, &self.context).await
    }

    /// Execute a tool with an optional context override.
    ///
    /// This is used for retrying tools with elevated sandbox policies.
    /// After execution, large results are routed through the workshop (#548).
    pub async fn execute_full_with_context(
        &self,
        name: &str,
        input: Value,
        context_override: Option<&ToolContext>,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::not_available(format!("tool '{name}' is not registered")))?;

        let ctx = context_override.unwrap_or(&self.context);
        let result = tool.execute(input.clone(), ctx).await?;

        // Large-output routing (#548): if the result exceeds the threshold and
        // the caller did not request `raw=true`, synthesise via the workshop.
        let raw_bypass = input.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);

        if let Some(router) = ctx.large_output_router.as_ref() {
            use crate::tools::large_output_router::{LargeOutputRouter, RouteDecision};
            match router.route(name, &result, raw_bypass) {
                RouteDecision::PassThrough => {}
                RouteDecision::Synthesise {
                    estimated_tokens,
                    threshold,
                } => {
                    // Store the raw output in the workshop variable store.
                    if let Some(vars_arc) = ctx.workshop_vars.as_ref() {
                        let mut vars = vars_arc.lock().await;
                        vars.store_raw(name, &result.content);
                    }

                    // Build a terse synthesis using the same model the registry
                    // was constructed for (workshop Flash model). For now we
                    // produce a structured header + truncated preview without
                    // a live API call so the engine stays dependency-free at
                    // the registry layer. A follow-up can wire in the Flash
                    // client when the async LLM call is safe here.
                    let preview_chars = 1_200usize;
                    let preview: String = result.content.chars().take(preview_chars).collect();
                    let ellipsis = if result.content.chars().count() > preview_chars {
                        "\n… [output truncated — full text in workshop variable `last_tool_result`]"
                    } else {
                        ""
                    };
                    let synthesis = format!("{preview}{ellipsis}");
                    let wrapped = LargeOutputRouter::wrap_synthesis(
                        name,
                        &synthesis,
                        estimated_tokens,
                        threshold,
                    );
                    tracing::debug!(
                        tool = name,
                        estimated_tokens,
                        threshold,
                        "large-output routed through workshop"
                    );
                    return Ok(ToolResult::success(wrapped));
                }
            }
        }

        Ok(result)
    }

    /// Get the current tool context.
    #[must_use]
    pub fn context(&self) -> &ToolContext {
        &self.context
    }

    /// Convert all tools to API Tool format for sending to the model.
    ///
    /// Output is sorted by tool name for **prefix-cache stability** (#263).
    /// Rust's `HashMap` uses a randomly-seeded hasher per process, so a raw
    /// `self.tools.values()` iteration emits tools in a different order on
    /// every `deepseek` launch, invalidating DeepSeek's KV prefix cache for
    /// every cross-session resume. Sorting here matches the way Claude Code
    /// stabilises its tool array (`assembleToolPool` in their reference).
    ///
    /// The serialised catalog is memoised on first call and pinned across
    /// reads so each tool's `description()` and `input_schema()` are sampled
    /// exactly once per registration. MCP adapters whose upstream description
    /// drifts on reconnect would otherwise rewrite the catalog mid-session
    /// and bust the prefix cache. The cache is invalidated on `register`,
    /// `remove`, and `clear`.
    #[must_use]
    pub fn to_api_tools(&self) -> Vec<Tool> {
        self.api_cache
            .get_or_init(|| self.build_api_tools())
            .clone()
    }

    fn build_api_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<&Arc<dyn ToolSpec>> = self.tools.values().collect();
        tools.sort_by(|a, b| a.name().cmp(b.name()));
        tools
            .into_iter()
            .filter(|tool| tool.model_visible())
            .map(|tool| {
                let mut schema = tool.input_schema();
                schema_sanitize::sanitize(&mut schema);
                schema_canonicalize::canonicalize_schema(&mut schema);
                Tool {
                    tool_type: None,
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    input_schema: schema,
                    allowed_callers: Some(vec!["direct".to_string()]),
                    defer_loading: Some(tool.defer_loading()),
                    input_examples: None,
                    strict: None,
                    cache_control: None,
                }
            })
            .collect()
    }

    fn invalidate_api_cache(&mut self) {
        self.api_cache = OnceLock::new();
    }

    /// Convert tools to API Tool format with optional cache control on the last tool.
    #[must_use]
    pub fn to_api_tools_with_cache(&self, enable_cache: bool) -> Vec<Tool> {
        let mut tools = self.to_api_tools();
        if enable_cache && let Some(last) = tools.last_mut() {
            last.cache_control = Some(crate::models::CacheControl {
                cache_type: "ephemeral".to_string(),
            });
        }
        tools
    }

    /// Filter tools by capability.
    #[must_use]
    pub fn filter_by_capability(&self, capability: ToolCapability) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| t.capabilities().contains(&capability))
            .cloned()
            .collect()
    }

    /// Get read-only tools.
    #[must_use]
    pub fn read_only_tools(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| t.is_read_only())
            .cloned()
            .collect()
    }

    /// Get tools that require approval.
    #[must_use]
    pub fn approval_required_tools(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| t.approval_requirement() == ApprovalRequirement::Required)
            .cloned()
            .collect()
    }

    /// Get tools that suggest approval.
    #[must_use]
    pub fn approval_suggested_tools(&self) -> Vec<Arc<dyn ToolSpec>> {
        self.tools
            .values()
            .filter(|t| {
                matches!(
                    t.approval_requirement(),
                    ApprovalRequirement::Suggest | ApprovalRequirement::Required
                )
            })
            .cloned()
            .collect()
    }

    /// Update the context (e.g., when workspace changes).
    pub fn set_context(&mut self, context: ToolContext) {
        self.context = context;
    }

    /// Get a mutable reference to the current context.
    #[must_use]
    pub fn context_mut(&mut self) -> &mut ToolContext {
        &mut self.context
    }

    /// Remove a tool by name.
    #[must_use]
    pub fn remove(&mut self, name: &str) -> Option<Arc<dyn ToolSpec>> {
        let removed = self.tools.remove(name);
        if removed.is_some() {
            self.invalidate_api_cache();
        }
        removed
    }

    /// Resolve a non-canonical tool name to a registered canonical name.
    ///
    /// Runs a deterministic ladder against the registered tool names:
    /// 1. Lowercase exact match.
    /// 2. Hyphens/spaces → underscores (read-file → read_file).
    /// 3. CamelCase → snake_case (ReadFile → read_file).
    /// 4. Strip trailing `_tool` / `-tool` suffix (twice).
    /// 5. Fuzzy match via simple prefix/suffix similarity.
    ///
    /// Returns `None` when no resolution is found (let the caller surface
    /// "Unknown tool").
    #[must_use]
    pub fn resolve(&self, requested: &str) -> Option<&str> {
        let names: Vec<&str> = self.tools.keys().map(String::as_str).collect();
        let lower = requested.to_lowercase();

        // 1. lowercase exact
        if let Some(n) = names.iter().find(|n| n.to_lowercase() == lower) {
            return Some(n);
        }
        // 2. hyphen/space → underscore
        let snaked = lower.replace(['-', ' '], "_");
        if let Some(n) = names.iter().find(|n| **n == snaked) {
            return Some(n);
        }
        // 3. CamelCase → snake_case
        let cc = to_snake_case(requested);
        if let Some(n) = names.iter().find(|n| **n == cc) {
            return Some(n);
        }
        // 4. strip _tool/-tool/tool suffix, twice
        let mut stripped = cc.clone();
        for _ in 0..2 {
            for suf in ["_tool", "-tool", "tool"] {
                if let Some(s) = stripped.strip_suffix(suf) {
                    stripped = s.to_string();
                    break;
                }
            }
        }
        if !stripped.is_empty()
            && let Some(n) = names.iter().find(|n| **n == stripped)
        {
            return Some(n);
        }
        // 5. fuzzy: simple prefix match (at least 3 chars)
        if lower.len() >= 3 {
            for n in &names {
                if n.len() >= 3 && (n.starts_with(&lower) || lower.starts_with(n)) {
                    return Some(n);
                }
            }
        }
        None
    }

    /// Clear all tools from the registry.
    pub fn clear(&mut self) {
        self.tools.clear();
        self.invalidate_api_cache();
    }

    /// Remove a tool from the registry by name. Returns `true` if the tool
    /// was present and removed, `false` if no tool with that name existed.
    pub fn remove_tool(&mut self, name: &str) -> bool {
        let existed = self.tools.remove(name).is_some();
        if existed {
            self.invalidate_api_cache();
        }
        existed
    }

    /// Apply config.toml tool overrides to this registry.
    ///
    /// For each entry in `overrides`:
    /// - `Disabled` removes the tool.
    /// - `Script` / `Command` replaces the tool with the user's implementation.
    ///
    /// `plugin_dir` is used as the base for relative script paths.
    pub fn apply_overrides(
        &mut self,
        overrides: &std::collections::HashMap<String, crate::config::ToolOverride>,
        plugin_dir: &Path,
    ) {
        for (tool_name, override_cfg) in overrides {
            match override_cfg {
                crate::config::ToolOverride::Disabled => {
                    if self.remove_tool(tool_name) {
                        tracing::info!("Tool '{}' disabled via config override", tool_name);
                    } else {
                        tracing::warn!("Cannot disable tool '{}': not registered", tool_name);
                    }
                }
                _ => {
                    // Script and Command overrides create replacement tools.
                    use crate::tools::plugin::tool_from_override;
                    match tool_from_override(tool_name, override_cfg, plugin_dir) {
                        Some(replacement) => {
                            self.register(replacement);
                            tracing::info!("Tool '{}' replaced via config override", tool_name);
                        }
                        None => {
                            if self.remove_tool(tool_name) {
                                tracing::warn!(
                                    "Tool '{}' override did not create a replacement; removed the original tool to avoid override fallthrough",
                                    tool_name
                                );
                            } else {
                                tracing::warn!(
                                    "Tool '{}' override did not create a replacement and no registered tool existed",
                                    tool_name
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Load and register plugin tools from a directory.
    ///
    /// Each script with valid frontmatter (`# name:`, `# description:`, etc.)
    /// becomes a registered `ScriptPluginTool`. Tools whose name matches an
    /// already-registered tool will overwrite it.
    pub fn load_plugins(&mut self, plugin_dir: &Path) {
        if !plugin_dir.exists() {
            tracing::debug!(
                "Plugin directory {} does not exist, skipping",
                plugin_dir.display()
            );
            return;
        }
        let plugins = crate::tools::plugin::load_plugin_tools(plugin_dir);
        let count = plugins.len();
        for tool in plugins {
            self.register(tool);
        }
        if count > 0 {
            tracing::info!(
                "Loaded {count} plugin tool(s) from {}",
                plugin_dir.display()
            );
        }
    }
}

/// Builder for constructing a `ToolRegistry` with common tools.
pub struct ToolRegistryBuilder {
    tools: Vec<Arc<dyn ToolSpec>>,
}

impl ToolRegistryBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Add a custom tool.
    #[must_use]
    pub fn with_tool(mut self, tool: Arc<dyn ToolSpec>) -> Self {
        self.tools.push(tool);
        self
    }

    #[must_use]
    pub fn with_dynamic_tools(mut self, dynamic_tools: &[DynamicToolSpec]) -> Self {
        for tool in dynamic_tools {
            self = self.with_tool(Arc::new(super::dynamic::RuntimeDynamicTool::new(
                tool.clone(),
            )));
        }
        self
    }

    /// Include file tools (read, write, edit, list).
    #[must_use]
    pub fn with_file_tools(self) -> Self {
        use super::file::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
        self.with_tool(Arc::new(ReadFileTool))
            .with_tool(Arc::new(WriteFileTool))
            .with_tool(Arc::new(EditFileTool))
            .with_tool(Arc::new(ListDirTool))
    }

    /// Include only read-only file tools (read, list).
    #[must_use]
    pub fn with_read_only_file_tools(self) -> Self {
        use super::file::{ListDirTool, ReadFileTool};
        self.with_tool(Arc::new(ReadFileTool))
            .with_tool(Arc::new(ListDirTool))
            .with_tool(Arc::new(
                super::tool_result_retrieval::RetrieveToolResultTool,
            ))
    }

    /// Include shell execution tool.
    #[must_use]
    pub fn with_shell_tools(self) -> Self {
        use super::shell::{ExecShellTool, ShellCancelTool, ShellInteractTool, ShellWaitTool};
        self.with_tool(Arc::new(ExecShellTool))
            .with_tool(Arc::new(ShellWaitTool::new("exec_shell_wait")))
            .with_tool(Arc::new(ShellInteractTool::new("exec_shell_interact")))
            .with_tool(Arc::new(ShellCancelTool))
    }

    /// Include search tools (`grep_files`).
    #[must_use]
    pub fn with_search_tools(self) -> Self {
        use super::file_search::FileSearchTool;
        use super::search::GrepFilesTool;
        self.with_tool(Arc::new(GrepFilesTool))
            .with_tool(Arc::new(FileSearchTool))
    }

    /// Include git inspection tools (`git_status`, `git_diff`) and the
    /// write operation `git_commit` (which requires user approval).
    #[must_use]
    pub fn with_git_tools(self) -> Self {
        use super::git::{GitCommitTool, GitDiffTool, GitStatusTool};
        self.with_tool(Arc::new(GitStatusTool))
            .with_tool(Arc::new(GitDiffTool))
            .with_tool(Arc::new(GitCommitTool))
    }

    /// Include git history tools (`git_log`, `git_show`, `git_blame`).
    #[must_use]
    pub fn with_git_history_tools(self) -> Self {
        use super::git_history::{GitBlameTool, GitLogTool, GitShowTool};
        self.with_tool(Arc::new(GitLogTool))
            .with_tool(Arc::new(GitShowTool))
            .with_tool(Arc::new(GitBlameTool))
    }

    /// Include workspace diagnostics tool.
    #[must_use]
    pub fn with_diagnostics_tool(self) -> Self {
        use super::diagnostics::DiagnosticsTool;
        self.with_tool(Arc::new(DiagnosticsTool))
    }

    /// 注册 LSP 符号导航工具（`lsp_document_symbols`、`lsp_find_references`、
    /// `lsp_goto_definition`）。三者都是只读的，运行时若 `ToolContext` 里没有
    /// `lsp_manager` 会返回明确错误，因此这里无条件注册即可。
    #[must_use]
    pub fn with_lsp_symbol_tools(self) -> Self {
        use super::lsp_symbols::{
            LspCallHierarchyTool, LspDocumentSymbolsTool, LspFindReferencesTool,
            LspGotoDefinitionTool,
        };
        self.with_tool(Arc::new(LspDocumentSymbolsTool))
            .with_tool(Arc::new(LspFindReferencesTool))
            .with_tool(Arc::new(LspGotoDefinitionTool))
            .with_tool(Arc::new(LspCallHierarchyTool))
    }

    /// 注册结构化 AST 检索工具（`ast_query`，issue #587）。只读、可并行，把
    /// tree-sitter 查询能力暴露给模型，替代手搓 grep/sed 做代码模式检索。
    #[must_use]
    pub fn with_ast_query_tool(self) -> Self {
        use super::ast_query::AstQueryTool;
        self.with_tool(Arc::new(AstQueryTool))
    }

    /// 注册调用图可达性工具（`call_graph`，issue #598 的 L1 基础能力）。只读、
    /// 可并行，把同文件调用图的传递闭包可达性暴露给模型，支持
    /// rust/java/tsx/javascript/kotlin/swift/objc（依构建 feature 而定）。
    #[must_use]
    pub fn with_call_graph_tool(self) -> Self {
        use super::call_graph::CallGraphTool;
        self.with_tool(Arc::new(CallGraphTool))
    }

    /// 注册 Hypothesis/Evidence/Verdict 跟踪工具（`hypothesis`，issue #803）。
    /// 在 `.mimofan/hypotheses.json` 维护推理账本，并强制「先举证后结论」
    /// 一致性门（零证据禁止 resolve），对应 vuln-hunt 长程 harness 的
    /// 推理严谨性轴（axis B）。只写 workspace 本地状态目录，不会执行代码或
    /// 发起网络请求，默认可自动批准，从而保持 harness 非交互。
    #[must_use]
    pub fn with_hypothesis_tools(self) -> Self {
        use super::hypothesis::HypothesisTool;
        self.with_tool(Arc::new(HypothesisTool))
    }

    /// 注册 gadget 链逆向追踪工具（`gadget_chain_trace`，issue #794 / #790
    /// 的静态可追踪性 axis）。只读、可并行，把漏洞知识库里的已知 gadget chain
    /// 反向追踪暴露给模型：给定 sink 符号与已确认存在的 gadget 集合，报告每条
    /// 利用链是否完整、以及还缺哪些 gadget（推动跨过程数据流分析）。
    #[must_use]
    pub fn with_gadget_chain_tools(self) -> Self {
        use super::gadget_chain::GadgetChainTraceTool;
        self.with_tool(Arc::new(GadgetChainTraceTool))
    }

    /// 注册可复现 PoC 执行工具（`run_poc`，issue #833 的 axis C 可复现性门）。
    /// 把候选 exploit 送进配置的 [`SandboxBackend`] 执行，并依据 `expect` 子串
    /// 判定漏洞是否被实际触发（`realized`）。无 sandbox backend 时决绝失败
    /// （fail-closed），对应 vuln-hunt 长程 harness 的可复现性轴。因执行代码，
    /// 默认可执行类、需审批。
    #[must_use]
    pub fn with_run_poc_tools(self) -> Self {
        use super::run_poc::RunPocTool;
        self.with_tool(Arc::new(RunPocTool))
    }

    /// Include the Jupyter notebook cell editing tool (`notebook_edit`).
    #[must_use]
    pub fn with_notebook_tools(self) -> Self {
        use super::notebook_edit::NotebookEditTool;
        self.with_tool(Arc::new(NotebookEditTool))
    }

    /// Append a batch of externally-supplied tools (e.g. resolved from a
    /// plugin manifest's `extra` tool list, issue #834 / plan W1). Each entry
    /// is registered through [`with_tool`], so behavior is identical to
    /// registering them one-by-one. An empty `extra` vec is a no-op.
    #[must_use]
    pub fn with_extra_tools(mut self, extra: Vec<Arc<dyn ToolSpec>>) -> Self {
        for tool in extra {
            self = self.with_tool(tool);
        }
        self
    }

    /// Include the `pandoc_convert` tool only when the `pandoc`
    /// binary is present on this host. Same probe-then-decide
    /// pattern v0.8.31 introduced for Python — when pandoc is
    /// missing the tool is not registered, so the model never
    /// sees a binary it can't actually use.
    #[must_use]
    pub fn with_pandoc_tools(self) -> Self {
        if crate::dependencies::resolve_pandoc().is_some() {
            use super::pandoc::PandocConvertTool;
            self.with_tool(Arc::new(PandocConvertTool))
        } else {
            self
        }
    }

    /// Include the `image_ocr` tool only when a local OCR backend is present.
    /// macOS uses the built-in Vision framework, while other platforms use
    /// Tesseract when installed.
    #[must_use]
    pub fn with_image_ocr_tools(self) -> Self {
        if super::image_ocr::ocr_available() {
            use super::image_ocr::ImageOcrTool;
            self.with_tool(Arc::new(ImageOcrTool))
        } else {
            self
        }
    }

    /// Include the `load_skill` tool (#434) so the model can pull a
    /// SKILL.md body + companion file list into context with one
    /// call instead of `read_file` + `list_dir` against the path
    /// shown in the system prompt's `## Skills` section.
    #[must_use]
    pub fn with_skill_tools(self) -> Self {
        use super::skill::LoadSkillTool;
        self.with_tool(Arc::new(LoadSkillTool))
    }

    /// Include project mapping tools.
    #[must_use]
    pub fn with_project_tools(self) -> Self {
        use super::project::ProjectMapTool;
        self.with_tool(Arc::new(ProjectMapTool))
    }

    /// Include cargo test runner tool.
    #[must_use]
    pub fn with_test_runner_tool(self) -> Self {
        use super::test_runner::RunTestsTool;
        use super::verifier::RunVerifiersTool;
        self.with_tool(Arc::new(RunTestsTool))
            .with_tool(Arc::new(RunVerifiersTool))
    }

    /// Include structured data validation tool (`validate_data`).
    #[must_use]
    pub fn with_validation_tools(self) -> Self {
        use super::validate_data::ValidateDataTool;
        self.with_tool(Arc::new(ValidateDataTool))
    }

    /// Include retrieval for spilled historical tool results.
    #[must_use]
    pub fn with_tool_result_retrieval_tool(self) -> Self {
        use super::tool_result_retrieval::RetrieveToolResultTool;
        self.with_tool(Arc::new(RetrieveToolResultTool))
    }

    /// Include durable task, gate, PR-attempt, GitHub, and automation tools.
    ///
    /// Shell-related task tools (`task_shell_start`, `task_shell_wait`) are
    /// *not* included here — use [`with_runtime_task_shell_tools`] to register
    /// them when `allow_shell` is true.
    #[must_use]
    pub fn with_runtime_task_tools(self) -> Self {
        use super::automation::{
            AutomationCreateTool, AutomationDeleteTool, AutomationListTool, AutomationPauseTool,
            AutomationReadTool, AutomationResumeTool, AutomationRunTool, AutomationUpdateTool,
        };
        use super::github::{
            GithubCloseIssueTool, GithubClosePrTool, GithubCommentTool, GithubIssueContextTool,
            GithubPrContextTool,
        };
        use super::tasks::{
            PrAttemptListTool, PrAttemptPreflightTool, PrAttemptReadTool, PrAttemptRecordTool,
            TaskCancelTool, TaskCreateTool, TaskGateRunTool, TaskListTool, TaskReadTool,
        };

        self.with_tool(Arc::new(TaskCreateTool))
            .with_tool(Arc::new(TaskListTool))
            .with_tool(Arc::new(TaskReadTool))
            .with_tool(Arc::new(TaskCancelTool))
            .with_tool(Arc::new(TaskGateRunTool))
            .with_tool(Arc::new(GithubIssueContextTool))
            .with_tool(Arc::new(GithubPrContextTool))
            .with_tool(Arc::new(PrAttemptRecordTool))
            .with_tool(Arc::new(PrAttemptListTool))
            .with_tool(Arc::new(PrAttemptReadTool))
            .with_tool(Arc::new(PrAttemptPreflightTool))
            .with_tool(Arc::new(AutomationCreateTool))
            .with_tool(Arc::new(AutomationListTool))
            .with_tool(Arc::new(AutomationReadTool))
            .with_tool(Arc::new(AutomationUpdateTool))
            .with_tool(Arc::new(AutomationPauseTool))
            .with_tool(Arc::new(AutomationResumeTool))
            .with_tool(Arc::new(AutomationDeleteTool))
            .with_tool(Arc::new(AutomationRunTool))
            .with_tool(Arc::new(GithubCommentTool))
            .with_tool(Arc::new(GithubCloseIssueTool))
            .with_tool(Arc::new(GithubClosePrTool))
    }

    /// Include shell-related task tools (`task_shell_start`, `task_shell_wait`, `task_shell_stop`).
    ///
    /// These are gated behind `allow_shell` because `task_shell_start`
    /// delegates directly to `ExecShellTool`, providing the same shell
    /// execution capability as `exec_shell`. `task_shell_stop` delegates to
    /// the shared background-shell kill path (`ShellCancelTool`).
    #[must_use]
    pub fn with_runtime_task_shell_tools(self) -> Self {
        use super::tasks::{TaskShellStartTool, TaskShellStopTool, TaskShellWaitTool};
        self.with_tool(Arc::new(TaskShellStartTool))
            .with_tool(Arc::new(TaskShellWaitTool))
            .with_tool(Arc::new(TaskShellStopTool))
    }

    /// Include only read-only durable task, PR-attempt, GitHub, and automation
    /// inspection tools. Plan mode uses this surface so it can observe state
    /// without starting work, changing remotes, or mutating automation config.
    #[must_use]
    pub fn with_runtime_read_only_task_tools(self) -> Self {
        use super::automation::{AutomationListTool, AutomationReadTool};
        use super::github::{GithubIssueContextTool, GithubPrContextTool};
        use super::tasks::{PrAttemptListTool, PrAttemptReadTool, TaskListTool, TaskReadTool};

        self.with_tool(Arc::new(TaskListTool))
            .with_tool(Arc::new(TaskReadTool))
            .with_tool(Arc::new(GithubIssueContextTool))
            .with_tool(Arc::new(GithubPrContextTool))
            .with_tool(Arc::new(PrAttemptListTool))
            .with_tool(Arc::new(PrAttemptReadTool))
            .with_tool(Arc::new(AutomationListTool))
            .with_tool(Arc::new(AutomationReadTool))
    }

    /// Include web search and fetch tools.
    ///
    /// These are feature-gated behind `Feature::WebSearch` in `tool_setup.rs`.
    /// `finance` is registered separately via `with_finance_tool()` and is
    /// NOT gated behind the web-search feature.
    #[must_use]
    pub fn with_web_tools(self) -> Self {
        use super::browser::BrowserTool;
        use super::dev_server_readiness::WaitForDevServerTool;
        use super::fetch_url::FetchUrlTool;
        use super::web_run::WebRunTool;
        use super::web_search::WebSearchTool;
        self.with_tool(Arc::new(WebSearchTool))
            .with_tool(Arc::new(FetchUrlTool))
            .with_tool(Arc::new(WaitForDevServerTool))
            .with_tool(Arc::new(WebRunTool))
            .with_tool(Arc::new(BrowserTool))
    }

    /// Include the `finance` market-data tool.
    ///
    /// This tool is registered unconditionally for agent modes and is NOT
    /// gated behind `Feature::WebSearch` (it fetches financial data, not
    /// web search results).
    #[must_use]
    pub fn with_finance_tool(self) -> Self {
        use super::finance::FinanceTool;
        self.with_tool(Arc::new(FinanceTool::new()))
    }

    /// Include the `insights` usage/cost analytics tool (issue #744).
    #[must_use]
    pub fn with_insights_tool(self) -> Self {
        use super::insights::InsightsTool;
        self.with_tool(Arc::new(InsightsTool))
    }

    /// Include the `synthetic_output` structured-output tool (issue #729).
    #[must_use]
    pub fn with_synthetic_output_tool(self) -> Self {
        use super::synthetic_output::SyntheticOutputTool;
        self.with_tool(Arc::new(SyntheticOutputTool))
    }

    /// Register the `image_analyze` vision tool.
    /// Only registered when `[vision_model]` is configured in config.toml.
    #[must_use]
    pub fn with_vision_tools(self, config: crate::config::VisionModelConfig) -> Self {
        use crate::vision::tools::ImageAnalyzeTool;
        self.with_tool(Arc::new(ImageAnalyzeTool::new(config)))
    }

    /// Include the `enter_worktree` / `exit_worktree` main-session tools (#697).
    #[must_use]
    pub fn with_worktree_tools(self) -> Self {
        use super::worktree::{EnterWorktreeTool, ExitWorktreeTool};
        self.with_tool(Arc::new(EnterWorktreeTool))
            .with_tool(Arc::new(ExitWorktreeTool))
    }

    /// Include the `create_sub_session` sibling-session tool (#697 item 1).
    #[must_use]
    pub fn with_create_sub_session_tool(self) -> Self {
        use super::create_sub_session::CreateSubSessionTool;
        self.with_tool(Arc::new(CreateSubSessionTool))
    }

    /// Include the `record_artifact` durable-artifact tool (#697 item 2).
    #[must_use]
    pub fn with_record_artifact_tool(self) -> Self {
        use super::record_artifact::RecordArtifactTool;
        self.with_tool(Arc::new(RecordArtifactTool))
    }

    /// Previously registered the OpenAI-style `multi_tool_use.parallel`
    /// meta-tool. DeepSeek-V4 has native parallel tool calls (multiple
    /// `tool_calls` entries in one assistant turn) and the meta-tool name
    /// triggered the model to hallucinate OpenAI-internal XML wrappers
    /// (`<multi_tool_use.parallel><tool_name>…</tool_name>…`) instead of
    /// emitting native calls. Kept as a no-op so existing callers compile;
    /// the engine's compatibility dispatcher still handles legacy emissions.
    ///
    /// Deprecated: this no longer registers anything. It is retained only so
    /// the three in-tree call sites (`tool_setup.rs`, `command_palette.rs`,
    /// `registry.rs`) keep compiling while the call sites are cleaned up. Do
    /// not add new callers — drop the `.with_parallel_tool()` chain link
    /// instead.
    #[deprecated(
        note = "no-op: native parallel tool calls are used instead. Drop this call-site link."
    )]
    #[must_use]
    pub fn with_parallel_tool(self) -> Self {
        self
    }

    /// Include request_user_input tool.
    #[must_use]
    pub fn with_user_input_tool(self) -> Self {
        use super::user_input::RequestUserInputTool;
        self.with_tool(Arc::new(RequestUserInputTool))
    }

    /// Include patch tools (`apply_patch`).
    #[must_use]
    pub fn with_patch_tools(self) -> Self {
        use super::apply_patch::ApplyPatchTool;
        self.with_tool(Arc::new(ApplyPatchTool))
    }

    /// Include the `revert_turn` tool. Approval-gated since it mutates
    /// the workspace; the model uses it when the user asks to "undo my
    /// last edit". Backed by the per-workspace snapshot side-repo
    /// (`crate::snapshot`).
    #[must_use]
    pub fn with_revert_turn_tool(self) -> Self {
        use super::revert_turn::RevertTurnTool;
        self.with_tool(Arc::new(RevertTurnTool))
    }

    /// Include Xiaomi MiMo speech/TTS tools (`speech`, `tts`).
    #[must_use]
    pub fn with_speech_tools(self, client: Option<ApiClient>, output_dir: Option<PathBuf>) -> Self {
        use super::speech::SpeechTool;
        self.with_tool(Arc::new(SpeechTool::new(
            "speech",
            client.clone(),
            output_dir.clone(),
        )))
        .with_tool(Arc::new(SpeechTool::new("tts", client, output_dir)))
    }

    /// Include persistent RLM session tools.
    #[must_use]
    pub fn with_rlm_tool(self, client: Option<ApiClient>, _root_model: String) -> Self {
        use super::rlm::{
            RlmCloseTool, RlmConfigureTool, RlmEvalTool, RlmOpenTool, RlmSessionObjectsTool,
        };
        self.with_tool(Arc::new(RlmSessionObjectsTool))
            .with_tool(Arc::new(RlmOpenTool))
            .with_tool(Arc::new(RlmEvalTool::new(client)))
            .with_tool(Arc::new(RlmConfigureTool))
            .with_tool(Arc::new(RlmCloseTool))
    }

    /// Include `handle_read`, the bounded projection reader for symbolic
    /// `var_handle` payloads.
    #[must_use]
    pub fn with_handle_tools(self) -> Self {
        use super::handle::HandleReadTool;
        self.with_tool(Arc::new(HandleReadTool))
    }

    /// Include the review tool.
    #[must_use]
    pub fn with_review_tool(self, client: Option<ApiClient>, model: String) -> Self {
        use super::review::ReviewTool;
        self.with_tool(Arc::new(ReviewTool::new(client, model)))
    }

    /// Include note tool.
    #[must_use]
    pub fn with_note_tool(self) -> Self {
        use super::shell::NoteTool;
        self.with_tool(Arc::new(NoteTool))
    }

    /// Include the FIM (Fill-in-the-Middle) edit tool.
    #[must_use]
    pub fn with_fim_tool(self, client: Option<ApiClient>, model: String) -> Self {
        use super::fim::FimEditTool;
        self.with_tool(Arc::new(FimEditTool::new(client, model)))
    }

    /// Include the `remember` tool — model-callable bullet-add into the
    /// user memory file (#489). Only register when the user has opted
    /// in to the memory feature; without that, the tool would surface
    /// in the model's catalog but always fail with "memory disabled".
    #[must_use]
    pub fn with_remember_tool(self) -> Self {
        use super::remember::RememberTool;
        self.with_tool(Arc::new(RememberTool))
    }

    /// Include the `remember_vector` tool — model-callable semantic memory
    /// write into the vector store (#570). Complements `remember` (file-based).
    /// Only registered when the embedding backend is configured
    /// (`MIMOFAN_MEMORY_API_KEY`); without that the tool would always fail.
    #[cfg(feature = "vector-memory")]
    #[must_use]
    pub fn with_remember_vector_tool(self) -> Self {
        use super::remember_vector::RememberVectorTool;
        self.with_tool(Arc::new(RememberVectorTool))
    }

    /// Include the slop ledger tools (#2127) — durable tracking of
    /// unresolved architectural residue: append, query, update, export.
    /// Registered unconditionally; the ledger JSON file is auto-created
    /// on first append.
    #[must_use]
    pub fn with_slop_ledger_tools(self) -> Self {
        use crate::slop_ledger::{
            SlopLedgerAppendTool, SlopLedgerExportTool, SlopLedgerQueryTool, SlopLedgerUpdateTool,
        };
        self.with_tool(Arc::new(SlopLedgerAppendTool))
            .with_tool(Arc::new(SlopLedgerQueryTool))
            .with_tool(Arc::new(SlopLedgerUpdateTool))
            .with_tool(Arc::new(SlopLedgerExportTool))
    }

    /// Read-only subset of slop ledger tools (#2127) for plan mode:
    /// only query and export — no append or update.
    #[must_use]
    pub fn with_slop_ledger_read_only_tools(self) -> Self {
        use crate::slop_ledger::{SlopLedgerExportTool, SlopLedgerQueryTool};
        self.with_tool(Arc::new(SlopLedgerQueryTool))
            .with_tool(Arc::new(SlopLedgerExportTool))
    }

    /// Include the `notify` tool — model-callable desktop notification
    /// (#1322). Routes through the existing `tui::notifications` OSC 9 /
    /// BEL pipeline so the user's `[notifications].method` config is
    /// honoured automatically (including `off`). Always safe to register
    /// because the tool has no side effects beyond a single terminal
    /// escape write.
    #[must_use]
    pub fn with_notify_tool(self) -> Self {
        use super::notify::NotifyTool;
        self.with_tool(Arc::new(NotifyTool))
    }

    /// Include MCP tools from a connected pool as first-class registry
    /// citizens. Each MCP tool is wrapped in a lightweight adapter that
    /// implements `ToolSpec`, so the unified `ToolRegistryBuilder` flow
    /// handles them alongside native tools.
    ///
    /// MCP tools are marked `defer_loading` by default (except discovery
    /// helpers) to keep the model-visible catalog compact.
    #[must_use]
    pub fn with_mcp_tools(
        mut self,
        mcp_pool: std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpPool>>,
    ) -> Self {
        // Snapshot the current tool list from the pool (non-blocking).
        // The adapter lazily resolves at execution time via the pool.
        if let Ok(pool) = mcp_pool.try_lock() {
            for (name, tool) in pool.all_tools() {
                let adapter = Arc::new(McpToolAdapter {
                    name: name.clone(),
                    tool: tool.clone(),
                    pool: mcp_pool.clone(),
                });
                self.tools.push(adapter);
            }
        }
        self
    }

    /// Include all agent tools under a typed shell policy.
    #[must_use]
    pub fn with_agent_tools_policy(self, shell_policy: crate::worker_profile::ShellPolicy) -> Self {
        let builder = self
            .with_file_tools()
            .with_note_tool()
            .with_search_tools()
            .with_user_input_tool()
            .with_git_tools()
            .with_git_history_tools()
            .with_diagnostics_tool()
            .with_lsp_symbol_tools()
            .with_hypothesis_tools()
            .with_project_tools()
            .with_skill_tools()
            .with_test_runner_tool()
            .with_validation_tools()
            .with_tool_result_retrieval_tool()
            .with_handle_tools()
            .with_runtime_task_tools()
            .with_revert_turn_tool()
            .with_notebook_tools()
            .with_pandoc_tools()
            .with_image_ocr_tools()
            .with_finance_tool()
            .with_insights_tool()
            .with_synthetic_output_tool()
            .with_worktree_tools()
            .with_create_sub_session_tool()
            .with_record_artifact_tool()
            .with_observability_tools();

        if shell_policy.allows_shell() {
            builder.with_shell_tools().with_runtime_task_shell_tools()
        } else {
            builder
        }
    }

    /// 注册可观测性/元认知工具族（#846/#847/#850）。三者都是 ReadOnly/Auto，
    /// 不会写文件或发网络请求，因此在无人值守安全子集里也安全：
    ///
    /// - `prompt_audit`：扫描 system prompt 的冗余/矛盾/膨胀，纯文本分析。
    /// - `advisor`：用确定性启发式给出模型路由决策（execute/escalate），不调用模型。
    /// - `event_stream`：只读暴露结构化事件日志路径（及可选的聚合计数），不写事件。
    ///
    /// `event_stream` 指向约定路径 `<workspace>/.mimofan/events.jsonl`（相对当前
    /// 工作区解析）；日志尚未创建或不可读时仍可正常报告路径（best-effort summary）。
    #[must_use]
    pub fn with_observability_tools(self) -> Self {
        use super::advisor::AdvisorTool;
        use super::prompt_audit::PromptAuditTool;
        use super::replay::EventStreamTool;

        let events_path = PathBuf::from(".mimofan").join("events.jsonl");

        self.with_tool(Arc::new(PromptAuditTool))
            .with_tool(Arc::new(AdvisorTool::new()))
            .with_tool(Arc::new(EventStreamTool::new(events_path)))
    }

    /// Include the full agent tool surface: every tool family the parent gets
    /// in Agent mode, including review, RLM, and the sub-agent management
    /// family (so children can recurse). Used by both the parent's Agent-mode
    /// registry build (`core/engine.rs`) and by every sub-agent
    /// (`subagent::SubAgentToolRegistry`) — keeping them in lockstep.
    ///
    /// `allow_shell` mirrors the session's shell permission. `manager` and
    /// `runtime` are the sub-agent runtime — children pass through their own
    /// runtime so grandchildren can spawn within the same depth/cancellation
    /// envelope.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn with_full_agent_surface(
        self,
        client: Option<ApiClient>,
        model: String,
        manager: super::subagent::SharedSubAgentManager,
        runtime: super::subagent::SubAgentRuntime,
        allow_shell: bool,
        todo_list: super::todo::SharedTodoList,
        plan_state: super::plan::SharedPlanState,
    ) -> Self {
        self.with_full_agent_surface_policy(
            client,
            model,
            manager,
            runtime,
            crate::worker_profile::ShellPolicy::from_legacy_allow_shell(allow_shell),
            todo_list,
            plan_state,
        )
    }

    /// Include the full agent surface under a typed shell policy.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn with_full_agent_surface_policy(
        self,
        client: Option<ApiClient>,
        model: String,
        manager: super::subagent::SharedSubAgentManager,
        runtime: super::subagent::SubAgentRuntime,
        shell_policy: crate::worker_profile::ShellPolicy,
        todo_list: super::todo::SharedTodoList,
        plan_state: super::plan::SharedPlanState,
    ) -> Self {
        let speech_client = client.clone();
        let speech_output_dir = runtime.speech_output_dir.clone();
        self.with_agent_tools_policy(shell_policy)
            .with_todo_tool(todo_list)
            .with_plan_tool(plan_state)
            .with_review_tool(client.clone(), model.clone())
            .with_rlm_tool(client, model)
            .with_speech_tools(speech_client, speech_output_dir)
            .with_subagent_tools(manager, runtime)
    }

    /// Include the todo tool with a shared `TodoList`.
    #[must_use]
    pub fn with_todo_tool(self, todo_list: super::todo::SharedTodoList) -> Self {
        use super::todo::{TodoAddTool, TodoClaimTool, TodoListTool, TodoUpdateTool, TodoWriteTool};
        self.with_tool(Arc::new(TodoWriteTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoAddTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoUpdateTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoListTool::new(todo_list.clone())))
            .with_tool(Arc::new(TodoClaimTool::new(todo_list, None)))
    }

    /// Include the plan tool with a shared `PlanState`.
    #[must_use]
    pub fn with_plan_tool(self, plan_state: super::plan::SharedPlanState) -> Self {
        use super::plan::UpdatePlanTool;
        self.with_tool(Arc::new(UpdatePlanTool::new(plan_state)))
    }

    /// Include the `exit_plan_mode` approval tool.
    ///
    /// Plan-mode only: outside Plan mode there is nothing to exit, and exposing
    /// it would invite the model to "approve" its way into work the user never
    /// gated.
    #[must_use]
    pub fn with_exit_plan_mode_tool(self) -> Self {
        use super::plan::ExitPlanModeTool;
        self.with_tool(Arc::new(ExitPlanModeTool))
    }

    /// Include runtime goal-queue tools: `goal_enqueue`, `goal_get`, `goal_update`,
    /// `goal_list`, `goal_pause`, `goal_resume`, `goal_cancel`, `goal_promote`.
    #[must_use]
    pub fn with_goal_tools(self, goal_queue: super::goal::SharedGoalQueue) -> Self {
        use super::goal::{
            GoalCancelTool, GoalEnqueueTool, GoalGetTool, GoalListTool, GoalPauseTool,
            GoalPromoteTool, GoalResumeTool, GoalUpdateTool,
        };
        self.with_tool(Arc::new(GoalEnqueueTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalGetTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalUpdateTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalListTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalPauseTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalResumeTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalCancelTool::new(goal_queue.clone())))
            .with_tool(Arc::new(GoalPromoteTool::new(goal_queue)))
    }

    /// Include sub-agent management tools.
    #[must_use]
    pub fn with_subagent_tools(
        self,
        manager: super::subagent::SharedSubAgentManager,
        runtime: super::subagent::SubAgentRuntime,
    ) -> Self {
        use super::subagent::AgentTool;
        use super::subagent::task_graph::TaskGraphTool;

        self.with_tool(Arc::new(AgentTool::new(manager.clone(), runtime.clone())))
            .with_tool(Arc::new(TaskGraphTool::new(manager, runtime)))
    }

    /// Include the declarative DAG `workflow` tool (#T-Q1). It drives the same
    /// shared sub-agent `manager` / `runtime` as the `agent` tool so each node
    /// is a real sub-agent that inherits the manager's token budget and can use
    /// worktree isolation. The workflow engine adds DAG scheduling, stall→retry,
    /// and journal resume on top of the existing dispatch path.
    #[must_use]
    pub fn with_workflow_tool(
        self,
        manager: super::subagent::SharedSubAgentManager,
        runtime: super::subagent::SubAgentRuntime,
    ) -> Self {
        use super::workflow::WorkflowTool;

        self.with_tool(Arc::new(WorkflowTool::new(manager, runtime)))
    }

    /// Build the registry with the given context.
    #[must_use]
    pub fn build(self, context: ToolContext) -> ToolRegistry {
        let mut registry = ToolRegistry::new(context);
        registry.register_all(self.tools);
        registry
    }
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert CamelCase to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Adapter that wraps an MCP tool definition so it can live in the
/// unified `ToolRegistry` alongside native tools (§5.B).
struct McpToolAdapter {
    name: String,
    tool: crate::mcp::McpTool,
    pool: std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpPool>>,
}

#[async_trait::async_trait]
impl ToolSpec for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        // McpTool.description is Option<String>; fall back to the
        // prefixed name when absent.
        self.tool.description.as_deref().unwrap_or(&self.name)
    }

    fn input_schema(&self) -> Value {
        self.tool.input_schema.clone()
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Conservatively treat MCP tools as requiring approval and
        // network access unless they're known discovery helpers.
        let name_lower = self.name.to_lowercase();
        if name_lower.contains("list_mcp")
            || name_lower.contains("read_mcp")
            || name_lower.contains("mcp_read")
            || name_lower.contains("mcp_get_prompt")
        {
            vec![ToolCapability::ReadOnly]
        } else {
            vec![ToolCapability::Network, ToolCapability::RequiresApproval]
        }
    }

    fn defer_loading(&self) -> bool {
        // Discovery helpers stay loaded; everything else is deferred.
        let keep_loaded = matches!(
            self.name.as_str(),
            "list_mcp_resources"
                | "list_mcp_resource_templates"
                | "mcp_read_resource"
                | "read_mcp_resource"
                | "mcp_get_prompt"
        );
        !keep_loaded
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let mut pool = self.pool.lock().await;
        let result = pool
            .call_tool(&self.name, input)
            .await
            .map_err(|e| ToolError::execution_failed(format!("MCP tool failed: {e}")))?;
        let content = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
        Ok(ToolResult::success(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    /// Minimal in-memory tool used to exercise the registry's bookkeeping
    /// without touching the network or the filesystem (issue #798).
    struct StubTool {
        name: &'static str,
        caps: Vec<ToolCapability>,
    }

    #[async_trait]
    impl ToolSpec for StubTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "stub tool for registry unit tests"
        }

        fn input_schema(&self) -> Value {
            json!({ "type": "object", "properties": {} })
        }

        fn capabilities(&self) -> Vec<ToolCapability> {
            self.caps.clone()
        }

        async fn execute(&self, _input: Value, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success(format!("ran {}", self.name)))
        }
    }

    fn stub(name: &'static str, caps: Vec<ToolCapability>) -> Arc<dyn ToolSpec> {
        Arc::new(StubTool { name, caps })
    }

    fn test_context() -> ToolContext {
        ToolContext::new(std::env::temp_dir().join("mimofan_registry_test_ws"))
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = ToolRegistry::new(test_context());
        assert!(reg.is_empty());
        reg.register(stub("alpha", vec![ToolCapability::ReadOnly]));

        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
        assert!(reg.contains("alpha"));
        assert!(!reg.contains("beta"));
        assert_eq!(reg.get("alpha").unwrap().name(), "alpha");
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn register_all_and_names() {
        let mut reg = ToolRegistry::new(test_context());
        reg.register_all(vec![
            stub("a", vec![]),
            stub("b", vec![]),
            stub("c", vec![]),
        ]);
        assert_eq!(reg.len(), 3);
        let mut names = reg.names();
        names.sort_unstable();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn re_register_overwrites() {
        let mut reg = ToolRegistry::new(test_context());
        reg.register(stub("dup", vec![ToolCapability::ReadOnly]));
        reg.register(stub("dup", vec![ToolCapability::Network]));
        // Name collision keeps a single entry but the latest tool wins.
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("dup").unwrap().capabilities(), vec![ToolCapability::Network]);
    }

    #[test]
    fn remove_and_clear() {
        let mut reg = ToolRegistry::new(test_context());
        reg.register(stub("x", vec![]));
        reg.register(stub("y", vec![]));
        assert!(reg.remove("x").is_some());
        assert!(reg.remove("x").is_none());
        assert!(reg.contains("y"));
        reg.clear();
        assert!(reg.is_empty());
    }

    #[test]
    fn filter_by_capability() {
        let mut reg = ToolRegistry::new(test_context());
        reg.register(stub("ro", vec![ToolCapability::ReadOnly]));
        reg.register(stub("net", vec![ToolCapability::Network]));
        reg.register(stub("both", vec![ToolCapability::ReadOnly, ToolCapability::Network]));

        let ro = reg.filter_by_capability(ToolCapability::ReadOnly);
        let mut ro_names: Vec<&str> = ro.iter().map(|t| t.name()).collect();
        ro_names.sort_unstable();
        assert_eq!(ro_names, vec!["both", "ro"]);

        let net = reg.filter_by_capability(ToolCapability::Network);
        let mut net_names: Vec<&str> = net.iter().map(|t| t.name()).collect();
        net_names.sort_unstable();
        assert_eq!(net_names, vec!["both", "net"]);
    }

    #[tokio::test]
    async fn execute_routes_to_registered_tool() {
        let mut reg = ToolRegistry::new(test_context());
        reg.register(stub("echo", vec![ToolCapability::ReadOnly]));

        let content = reg.execute("echo", json!({})).await.unwrap();
        assert_eq!(content, "ran echo");

        let err = reg.execute("missing", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("not registered"));
    }

    /// Smoke test: the central observability wiring actually registers the
    /// new read-only tools into the default agent tool set (#846/#847/#850).
    #[test]
    fn observability_tools_registered_in_agent_surface() {
        // Build a minimal agent surface builder and assert the new tools are
        // present. We don't need a live shell policy to check presence.
        let reg = ToolRegistryBuilder::new()
            .with_agent_tools_policy(crate::worker_profile::ShellPolicy::from_legacy_allow_shell(false))
            .build(test_context());

        for name in ["prompt_audit", "advisor", "event_stream"] {
            assert!(reg.contains(name), "agent surface is missing tool `{name}`");
            // They must stay read-only so the unattended safety filter keeps them.
            assert!(
                reg.get(name).unwrap().is_read_only(),
                "`{name}` must be ReadOnly"
            );
        }
    }
}

// === Unit Tests ===
