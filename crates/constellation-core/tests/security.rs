use constellation_core::{
    AgentId, ChatCompletionRequest, ChatEntry, ChatRole, CoreError, CredentialSource,
    OpenAiCompatibleAdapter, PermissionGate, PermissionScope, ProviderAdapter, ProviderId,
    ProviderProfile, RoomId, StudioStore, TaskId, ToolAction,
};

fn scope(action: ToolAction) -> PermissionScope {
    PermissionScope {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
        action,
        resource: "/private/research.txt".to_owned(),
    }
}

#[test]
fn no_actions_execute_without_scoped_explicit_permission() {
    let mut gate = PermissionGate::new();
    let request = scope(ToolAction::ExecuteCommand);
    assert!(matches!(
        gate.authorize(&request),
        Err(CoreError::PermissionDenied)
    ));
    gate.grant(request.clone()).unwrap();
    assert!(gate.authorize(&request).is_ok());
    let mut other = request.clone();
    other.task_id = TaskId::new();
    assert!(matches!(
        gate.authorize(&other),
        Err(CoreError::PermissionDenied)
    ));
    other = request.clone();
    other.agent_id = AgentId::new();
    assert!(matches!(
        gate.authorize(&other),
        Err(CoreError::PermissionDenied)
    ));
    other = request.clone();
    other.room_id = RoomId::new();
    assert!(matches!(
        gate.authorize(&other),
        Err(CoreError::PermissionDenied)
    ));
    other = request.clone();
    other.action = ToolAction::ReadFile;
    assert!(matches!(
        gate.authorize(&other),
        Err(CoreError::PermissionDenied)
    ));
    other = request.clone();
    other.resource = "/private/other.txt".into();
    assert!(matches!(
        gate.authorize(&other),
        Err(CoreError::PermissionDenied)
    ));
    assert!(gate.revoke(&request));
    assert!(matches!(
        gate.authorize(&request),
        Err(CoreError::PermissionDenied)
    ));
}

fn profile(endpoint: &str) -> ProviderProfile {
    ProviderProfile {
        id: ProviderId::new(),
        display_name: "Custom".into(),
        model_id: "my-model".into(),
        base_url: endpoint.into(),
        credentials: CredentialSource::Environment {
            variable: "CONSTELLATION_PROVIDER_API_KEY".into(),
        },
    }
}

#[test]
fn secure_endpoints_and_openai_compatible_request() {
    for invalid in [
        "http://api.example.com/v1",
        "http://192.168.1.1/v1",
        "https://key:secret@api.example.com/v1",
        "https://api.example.com/v1?token=secret",
        "ftp://example.com/v1",
        "not a URL",
    ] {
        assert!(
            profile(invalid).validate().is_err(),
            "accepted insecure endpoint {invalid}"
        );
    }
    for valid in [
        "https://api.example.com/v1",
        "http://localhost:11434/v1",
        "http://127.0.0.1:1234/v1/",
        "http://[::1]:9000/v1",
    ] {
        assert!(
            profile(valid).validate().is_ok(),
            "rejected permitted endpoint {valid}"
        );
    }

    let adapter = OpenAiCompatibleAdapter::new(profile("https://example.com/v1/")).unwrap();
    let prepared = adapter
        .prepare_chat(&ChatCompletionRequest {
            messages: vec![ChatEntry {
                role: ChatRole::User,
                content: "hello".into(),
            }],
        })
        .unwrap();
    assert_eq!(
        prepared.endpoint.as_str(),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(prepared.body["model"], "my-model");
    assert_eq!(prepared.body["messages"][0]["role"], "user");
    assert!(!format!("{prepared:?}").contains("Bearer"));
}

#[test]
fn provider_profiles_only_accept_credential_references_and_persist_them() {
    let raw = serde_json::json!({
        "id": ProviderId::new(),
        "display_name": "Hosted",
        "model_id": "model",
        "base_url": "https://example.com/v1",
        "credentials": {"kind": "environment", "variable": "API_KEY"},
        "api_key": "sk-plaintext-should-not-be-stored"
    });
    assert!(serde_json::from_value::<ProviderProfile>(raw).is_err());
    let profile = profile("https://example.com/v1");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("studio.sqlite");
    let store = StudioStore::open(&path).unwrap();
    store.save_provider_profile(profile.clone()).unwrap();
    assert_eq!(store.snapshot().unwrap().providers, vec![profile]);
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.windows(4).any(|w| w == b"sk-"));
}
