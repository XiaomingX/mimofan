// Tests relocated from src/client.rs (issue #547 Phase 3).

    use mimofan::client::*;
    use serde_json::json;
    use mimofan::models::MessageRequest;
    
    
    use mimofan::config::ApiProvider;

    #[test]
    fn xiaomi_mimo_anthropic_base_url_picks_messages_protocol() {
        // ~/.mimofan/config.toml providers.xiaomi_mimo.base_url =
        // `https://api.xiaomimimo.com/anthropic` must dispatch to the
        // Anthropic Messages client, not the Responses client.
        assert!(api_provider_uses_anthropic_messages(
            ApiProvider::XiaomiMimo,
            "https://api.xiaomimimo.com/anthropic"
        ));
    }

    #[test]
    fn xiaomi_mimo_anthropic_base_url_with_trailing_slash() {
        assert!(api_provider_uses_anthropic_messages(
            ApiProvider::XiaomiMimo,
            "https://api.xiaomimimo.com/anthropic/"
        ));
    }

    #[test]
    fn xiaomi_mimo_token_plan_base_url_uses_chat_completions_dialect() {
        // Pay-as-you-go token-plan endpoint keeps the OpenAI Chat
        // Completions dialect. The Codex Responses API in
        // `client/responses.rs` is *not* served by the XiaomiMiMo
        // gateway — dispatch must fall through to the Chat path instead.
        assert!(!api_provider_uses_anthropic_messages(
            ApiProvider::XiaomiMimo,
            "https://token-plan-sgp.xiaomimimo.com/v1"
        ));
    }

    #[test]
    fn custom_provider_with_anthropic_url_uses_messages_api() {
        // Custom providers with /anthropic base URL should use Anthropic Messages API.
        let provider = ApiProvider::Custom;
        assert!(
            api_provider_uses_anthropic_messages(provider, "https://api.xiaomimimo.com/anthropic"),
            "{provider:?} with /anthropic URL should dispatch through Anthropic Messages"
        );
    }

    #[test]
    fn custom_provider_with_v1_url_uses_chat_completions() {
        // Custom providers with /v1 base URL should use OpenAI Chat Completions API.
        let provider = ApiProvider::Custom;
        assert!(
            !api_provider_uses_anthropic_messages(provider, "https://api.xiaomimimo.com/v1"),
            "{provider:?} with /v1 URL should NOT dispatch through Anthropic Messages"
        );
    }

    #[test]
    fn xiaomi_mimo_with_v1_url_uses_chat_completions() {
        // XiaomiMimo with /v1 base URL should use OpenAI Chat Completions API.
        assert!(!api_provider_uses_anthropic_messages(
            ApiProvider::XiaomiMimo,
            "https://api.xiaomimimo.com/v1"
        ));
    }

    // ── MessageRequest::response_format round-trip (#0.0.3-rc.3) ──────────
    // The OpenAI Chat Completions body builder forwards `response_format`
    // (e.g. XiaomiMiMo `{"type":"json_object"}` JSON mode). Lock in the
    // field's serde shape so downstream `serde_json::from_value` of a
    // caller-supplied body still parses.
    #[test]
    fn message_request_response_format_round_trips() {
        let rf = json!({ "type": "json_object" });
        let req = MessageRequest {
            model: "mimo-v2.5-pro".to_string(),
            messages: vec![],
            max_tokens: 256,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
            response_format: Some(rf.clone()),
        };
        let value = serde_json::to_value(&req).expect("serialize");
        assert_eq!(value["response_format"], rf);
        let parsed: MessageRequest = serde_json::from_value(value).expect("deserialize");
        assert_eq!(parsed.response_format.as_ref(), Some(&rf));
    }

    #[test]
    fn message_request_response_format_omitted_when_none() {
        // `skip_serializing_if = "Option::is_none"` keeps the wire body
        // clean for callers that don't opt in.
        let req = MessageRequest {
            model: "mimo-v2.5-pro".to_string(),
            messages: vec![],
            max_tokens: 256,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
            response_format: None,
        };
        let value = serde_json::to_value(&req).expect("serialize");
        assert!(value.get("response_format").is_none());
    }
