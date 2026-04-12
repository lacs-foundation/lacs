//! Read-only query tools for the planning phase.
//!
//! These tools let the LLM gather specific system information before
//! proposing a plan. Each maps to a Low-risk daemon action.

use crate::provider::ToolDefinition;

pub fn query_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "query_services".into(),
            description: "List all running systemd services. Returns one service name per line."
                .into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_firewall".into(),
            description: "Show current firewall rules and allowed services.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_deployments".into(),
            description:
                "List all rpm-ostree deployments with their index, version, and pinned status."
                    .into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_packages".into(),
            description: "List all layered packages installed via rpm-ostree.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_containers".into(),
            description: "List all running containers (podman) with name and status.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
        ToolDefinition {
            name: "query_users".into(),
            description: "List local user accounts (uid >= 1000) with username and groups.".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
    ]
}

/// Map a query tool name to the corresponding daemon action name and params.
///
/// Returns `None` for tool names that are not query tools.
pub fn query_tool_to_action(tool_name: &str) -> Option<(&'static str, serde_json::Value)> {
    match tool_name {
        "query_services" => Some(("ListServices", serde_json::json!({}))),
        "query_firewall" => Some(("GetFirewallState", serde_json::json!({}))),
        "query_deployments" => Some(("ListDeployments", serde_json::json!({}))),
        "query_packages" => Some(("GetLayeredPackages", serde_json::json!({}))),
        "query_containers" => Some(("ListContainers", serde_json::json!({}))),
        "query_users" => Some(("ListUsers", serde_json::json!({}))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_query_tools_map_to_actions() {
        assert_eq!(
            query_tool_to_action("query_services"),
            Some(("ListServices", serde_json::json!({})))
        );
        assert_eq!(
            query_tool_to_action("query_firewall"),
            Some(("GetFirewallState", serde_json::json!({})))
        );
        assert_eq!(
            query_tool_to_action("query_deployments"),
            Some(("ListDeployments", serde_json::json!({})))
        );
        assert_eq!(
            query_tool_to_action("query_packages"),
            Some(("GetLayeredPackages", serde_json::json!({})))
        );
        assert_eq!(
            query_tool_to_action("query_containers"),
            Some(("ListContainers", serde_json::json!({})))
        );
        assert_eq!(
            query_tool_to_action("query_users"),
            Some(("ListUsers", serde_json::json!({})))
        );
    }

    #[test]
    fn unknown_query_tool_returns_none() {
        assert!(query_tool_to_action("query_unknown").is_none());
        assert!(query_tool_to_action("propose_plan").is_none());
    }

    #[test]
    fn query_tools_returns_six_definitions() {
        let tools = query_tools();
        assert_eq!(tools.len(), 6);
        for tool in &tools {
            assert!(tool.name.starts_with("query_"));
            assert!(!tool.description.is_empty());
        }
    }
}
