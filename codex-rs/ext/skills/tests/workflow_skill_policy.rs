use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::ConversationHistory;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolPayload;
use codex_extension_api::TurnInputContext;
use codex_extension_api::WorldStateContributionInput;
use codex_mcp::CODEX_APPS_MCP_SERVER_NAME;
use codex_protocol::capabilities::CapabilityRootLocation;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TruncationPolicy;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_protocol::user_input::UserInput;
use codex_skills_extension::SkillIdentity;
use codex_skills_extension::SkillProviders;
use codex_skills_extension::SkillVisibilityPolicy;
use codex_skills_extension::SkillsExtensionConfig;
use codex_skills_extension::catalog::SkillAuthority;
use codex_skills_extension::catalog::SkillCatalog;
use codex_skills_extension::catalog::SkillCatalogEntry;
use codex_skills_extension::catalog::SkillPackageId;
use codex_skills_extension::catalog::SkillProviderResult;
use codex_skills_extension::catalog::SkillReadResult;
use codex_skills_extension::catalog::SkillResourceId;
use codex_skills_extension::catalog::SkillSearchResult;
use codex_skills_extension::catalog::SkillSourceKind;
use codex_skills_extension::install_with_providers;
use codex_skills_extension::provider::SkillListQuery;
use codex_skills_extension::provider::SkillProvider;
use codex_skills_extension::provider::SkillProviderFuture;
use codex_skills_extension::provider::SkillReadRequest;
use codex_skills_extension::provider::SkillSearchRequest;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn workflow_skill_policy_filters_every_authority_and_access_path() -> TestResult {
    let host = authority(SkillSourceKind::Host, "host");
    let executor = authority(SkillSourceKind::Executor, "env-1");
    let orchestrator = authority(SkillSourceKind::Orchestrator, CODEX_APPS_MCP_SERVER_NAME);
    let host_provider = TestProvider::new(catalog_for(&host, "host"));
    let executor_provider = TestProvider::new(catalog_for(&executor, "executor"));
    let orchestrator_provider = TestProvider::new(catalog_for(&orchestrator, "orchestrator"));
    let providers = SkillProviders::new()
        .with_host_provider(Arc::new(host_provider.clone()))
        .with_executor_provider(Arc::new(executor_provider.clone()))
        .with_orchestrator_provider(Arc::new(orchestrator_provider.clone()));

    let mut builder = ExtensionRegistryBuilder::new();
    install_with_providers(&mut builder, providers, |_| SkillsExtensionConfig {
        include_instructions: true,
        bundled_skills_enabled: true,
        orchestrator_skills_enabled: true,
        shadow_selection_enabled: false,
    });
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(SkillVisibilityPolicy::allow_only([
        identity(&host, "host/allowed"),
        identity(&executor, "executor/allowed"),
        identity(&orchestrator, "orchestrator/allowed"),
    ]));
    registry.thread_lifecycle_contributors()[0]
        .on_thread_start(ThreadStartInput {
            config: &(),
            session_source: &SessionSource::Cli,
            persistent_thread_state_available: true,
            environments: &[],
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;

    let thread_fragments = registry.context_contributors()[0]
        .contribute_thread_context(&session_store, &thread_store)
        .await;
    assert_eq!(1, thread_fragments.len());
    assert!(thread_fragments[0].text().contains("orchestrator-allowed"));
    assert!(!thread_fragments[0].text().contains("orchestrator-blocked"));

    let turn_store = ExtensionData::new("turn-1");
    let selected_roots = vec![SelectedCapabilityRoot {
        id: "workflow-skills".to_string(),
        location: CapabilityRootLocation::Environment {
            environment_id: "env-1".to_string(),
            path: PathUri::parse("file:///skills").expect("skill root URI"),
        },
    }];
    let turn_environment = TurnEnvironmentSelection {
        environment_id: "env-1".to_string(),
        cwd: PathUri::parse("file:///workspace").expect("cwd URI"),
    };
    let world_state = registry.context_contributors()[0]
        .contribute_world_state(WorldStateContributionInput {
            thread_id: codex_protocol::ThreadId::new(),
            turn_id: "turn-1",
            environments: std::slice::from_ref(&turn_environment),
            ready_selected_capability_roots: &selected_roots,
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &turn_store,
        })
        .await;
    assert_eq!(1, world_state.len());
    let executor_fragment = world_state[0]
        .render_diff(codex_extension_api::PreviousWorldStateSection::Absent)
        .ok_or("allowed executor skill should render")?;
    assert!(executor_fragment.body().contains("executor-allowed"));
    assert!(!executor_fragment.body().contains("executor-blocked"));

    let turn_fragments = registry.turn_input_contributors()[0]
        .contribute(
            TurnInputContext {
                turn_id: "turn-1".to_string(),
                user_input: vec![UserInput::Text {
                    text: [
                        "$host-allowed",
                        "$host-blocked",
                        "$executor-allowed",
                        "$executor-blocked",
                        "$orchestrator-allowed",
                        "$orchestrator-blocked",
                    ]
                    .join(" "),
                    text_elements: Vec::new(),
                }],
                environments: Vec::new(),
            },
            &session_store,
            &thread_store,
            &turn_store,
        )
        .await;
    let rendered_turn = turn_fragments
        .iter()
        .map(|fragment| fragment.render())
        .collect::<Vec<_>>()
        .join("\n");
    for name in ["host-allowed", "executor-allowed", "orchestrator-allowed"] {
        assert!(rendered_turn.contains(name));
    }
    for name in ["host-blocked", "executor-blocked", "orchestrator-blocked"] {
        assert!(!rendered_turn.contains(name));
    }
    assert_eq!(
        vec!["host/allowed".to_string()],
        host_provider.read_packages()
    );
    assert_eq!(
        vec!["executor/allowed".to_string()],
        executor_provider.read_packages()
    );
    assert_eq!(
        vec!["orchestrator/allowed".to_string()],
        orchestrator_provider.read_packages()
    );

    let tools = registry.tool_contributors()[0].tools(&session_store, &thread_store);
    let list_tool = tools
        .iter()
        .find(|tool| tool.tool_name().name == "list")
        .ok_or("skills.list tool should be registered")?;
    let list_payload = ToolPayload::Function {
        arguments: serde_json::json!({"authority": {"kind": "orchestrator"}}).to_string(),
    };
    let list_output = list_tool
        .handle(tool_call(
            "list-call",
            list_tool.tool_name(),
            list_payload.clone(),
        ))
        .await?;
    let list_response = list_output
        .post_tool_use_response("list-call", &list_payload)
        .ok_or("skills.list should return structured output")?;
    assert_eq!(
        serde_json::json!([{
            "authority": {"kind": "orchestrator"},
            "package": "orchestrator/allowed",
            "name": "orchestrator-allowed",
            "description": "Test skill.",
            "main_resource": "skill://orchestrator/allowed/SKILL.md",
        }]),
        list_response["skills"]
    );

    let read_tool = tools
        .iter()
        .find(|tool| tool.tool_name().name == "read")
        .ok_or("skills.read tool should be registered")?;
    let blocked_payload = ToolPayload::Function {
        arguments: serde_json::json!({
            "authority": {"kind": "orchestrator"},
            "package": "orchestrator/blocked",
            "resource": "skill://orchestrator/blocked/SKILL.md",
        })
        .to_string(),
    };
    assert!(
        read_tool
            .handle(tool_call(
                "blocked-read",
                read_tool.tool_name(),
                blocked_payload
            ))
            .await
            .is_err()
    );
    assert_eq!(
        vec!["orchestrator/allowed".to_string()],
        orchestrator_provider.read_packages()
    );

    Ok(())
}

#[tokio::test]
async fn workflow_skill_policy_disable_all_hides_catalog_and_prevents_explicit_reads() -> TestResult
{
    let host = authority(SkillSourceKind::Host, "host");
    let provider = TestProvider::new(catalog_for(&host, "host"));
    let mut builder = ExtensionRegistryBuilder::new();
    install_with_providers(
        &mut builder,
        SkillProviders::new().with_host_provider(Arc::new(provider.clone())),
        |_| SkillsExtensionConfig {
            include_instructions: true,
            bundled_skills_enabled: true,
            orchestrator_skills_enabled: false,
            shadow_selection_enabled: false,
        },
    );
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(SkillVisibilityPolicy::DisableAll);
    registry.thread_lifecycle_contributors()[0]
        .on_thread_start(ThreadStartInput {
            config: &(),
            session_source: &SessionSource::Cli,
            persistent_thread_state_available: true,
            environments: &[],
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;

    let fragments = registry.turn_input_contributors()[0]
        .contribute(
            TurnInputContext {
                turn_id: "turn-1".to_string(),
                user_input: vec![UserInput::Text {
                    text: "$host-allowed".to_string(),
                    text_elements: Vec::new(),
                }],
                environments: Vec::new(),
            },
            &session_store,
            &thread_store,
            &ExtensionData::new("turn-1"),
        )
        .await;
    assert!(fragments.is_empty());
    assert!(provider.read_packages().is_empty());

    Ok(())
}

fn authority(kind: SkillSourceKind, id: &str) -> SkillAuthority {
    SkillAuthority::new(kind, id)
}

fn identity(authority: &SkillAuthority, package: &str) -> SkillIdentity {
    SkillIdentity::new(authority.clone(), SkillPackageId(package.to_string()))
}

fn catalog_for(authority: &SkillAuthority, prefix: &str) -> SkillCatalog {
    SkillCatalog {
        entries: ["allowed", "blocked"]
            .into_iter()
            .map(|suffix| {
                let package = format!("{prefix}/{suffix}");
                SkillCatalogEntry::new(
                    SkillPackageId(package.clone()),
                    authority.clone(),
                    format!("{prefix}-{suffix}"),
                    "Test skill.",
                    SkillResourceId::new(format!("skill://{package}/SKILL.md")),
                )
            })
            .collect(),
        warnings: Vec::new(),
    }
}

fn tool_call(
    call_id: &str,
    tool_name: codex_extension_api::ToolName,
    payload: ToolPayload,
) -> ToolCall {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: call_id.to_string(),
        tool_name,
        model: "gpt-test".to_string(),
        truncation_policy: TruncationPolicy::Bytes(4_096),
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload,
    }
}

#[derive(Clone)]
struct TestProvider {
    catalog: SkillCatalog,
    reads: Arc<Mutex<Vec<SkillReadRequest>>>,
}

impl TestProvider {
    fn new(catalog: SkillCatalog) -> Self {
        Self {
            catalog,
            reads: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn read_packages(&self) -> Vec<String> {
        self.reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|request| request.package.0.clone())
            .collect()
    }
}

impl SkillProvider for TestProvider {
    fn list(&self, _query: SkillListQuery) -> SkillProviderFuture<'_, SkillCatalog> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn read(&self, request: SkillReadRequest) -> SkillProviderFuture<'_, SkillReadResult> {
        let reads = Arc::clone(&self.reads);
        Box::pin(async move {
            reads
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            Ok(SkillReadResult {
                resource: request.resource,
                contents: format!("# {}", request.package.0),
            })
        })
    }

    fn search(&self, _request: SkillSearchRequest) -> SkillProviderFuture<'_, SkillSearchResult> {
        Box::pin(async { SkillProviderResult::Ok(SkillSearchResult::default()) })
    }
}
