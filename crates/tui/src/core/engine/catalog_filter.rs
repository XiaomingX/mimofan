use crate::models::Tool;

use super::turn_loop;

pub(crate) const MAX_PARALLEL_SHELL_EXEC: usize = 4;

pub(crate) fn default_active_native_tool_names() -> &'static [&'static str] {
    super::tool_catalog::DEFAULT_ACTIVE_NATIVE_TOOLS
}

/// Drop catalog entries the execution gates would reject (#3027): the model
/// should never be advertised a tool it cannot call. Deny wins over allow.
pub(crate) fn filter_tool_catalog_for_gates(
    catalog: &mut Vec<Tool>,
    allowed_tools: Option<&[String]>,
    disallowed_tools: Option<&[String]>,
) {
    catalog.retain(|tool| {
        !turn_loop::command_denies_tool(disallowed_tools, &tool.name)
            && turn_loop::command_allows_tool(allowed_tools, &tool.name)
    });
}
