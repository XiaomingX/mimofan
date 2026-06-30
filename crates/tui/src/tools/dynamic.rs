use async_trait::async_trait;
use mimofan_protocol::runtime::DynamicToolSpec;
use serde_json::Value;

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

pub struct RuntimeDynamicTool {
    spec: DynamicToolSpec,
}

impl RuntimeDynamicTool {
    pub fn new(spec: DynamicToolSpec) -> Self {
        Self { spec }
    }
}

#[async_trait]
impl ToolSpec for RuntimeDynamicTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn description(&self) -> &str {
        &self.spec.description
    }

    fn input_schema(&self) -> Value {
        self.spec.input_schema.clone()
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        false
    }

    fn defer_loading(&self) -> bool {
        self.spec.defer_loading
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let executor = context
            .runtime
            .dynamic_tool_executor
            .as_ref()
            .ok_or_else(|| {
                ToolError::not_available(format!(
                    "runtime dynamic tool '{}' has no executor",
                    self.spec.name
                ))
            })?;
        executor
            .execute_dynamic_tool(
                context.runtime.active_thread_id.clone(),
                self.spec.namespace.clone(),
                self.spec.name.clone(),
                input,
            )
            .await
    }
}

#[cfg(test)]
mod tests {}
