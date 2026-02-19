use std::collections::HashMap;
use std::sync::Arc;

use golem::engine::Engine;
use golem::engine::react::{ReactConfig, ReactEngine};
use golem::memory::sqlite::SqliteMemory;
use golem::thinker::mock::MockThinker;
use golem::thinker::{Step, StepResult, Thinker, ToolCall};
use golem::tools::ToolRegistry;
use golem::tools::shell::{ShellConfig, ShellMode, ShellTool};

/// Wrap steps into StepResults with no token usage (convenience for tests).
fn wrap(steps: Vec<Step>) -> Vec<StepResult> {
    steps
        .into_iter()
        .map(|step| StepResult { step, usage: None })
        .collect()
}

async fn build_engine(steps: Vec<Step>) -> ReactEngine {
    let thinker = Box::new(MockThinker::new(wrap(steps)));
    let tools = Arc::new(ToolRegistry::new());
    tools
        .register(Arc::new(ShellTool::new(ShellConfig {
            mode: ShellMode::ReadWrite,
            working_dir: std::env::current_dir().unwrap(),
            require_confirmation: false,
            ..ShellConfig::default()
        })))
        .await;
    let memory = Box::new(SqliteMemory::in_memory().unwrap());
    ReactEngine::new(thinker, tools, memory, ReactConfig::default())
}

#[tokio::test]
async fn finish_immediately() {
    let mut engine = build_engine(vec![Step::Finish {
        thought: "nothing to do".to_string(),
        answer: "done".to_string(),
    }])
    .await;

    let result = engine.run("do nothing").await.unwrap();
    assert_eq!(result, "done");
}

#[tokio::test]
async fn single_tool_call_then_finish() {
    let mut engine = build_engine(vec![
        Step::Act {
            thought: "let me check".to_string(),
            calls: vec![ToolCall {
                tool: "shell".to_string(),
                args: HashMap::from([("command".to_string(), "echo hello".to_string())]),
            }],
        },
        Step::Finish {
            thought: "got it".to_string(),
            answer: "hello".to_string(),
        },
    ])
    .await;

    let result = engine.run("say hello").await.unwrap();
    assert_eq!(result, "hello");
}

#[tokio::test]
async fn parallel_tool_calls() {
    let mut engine = build_engine(vec![
        Step::Act {
            thought: "run both at once".to_string(),
            calls: vec![
                ToolCall {
                    tool: "shell".to_string(),
                    args: HashMap::from([("command".to_string(), "echo one".to_string())]),
                },
                ToolCall {
                    tool: "shell".to_string(),
                    args: HashMap::from([("command".to_string(), "echo two".to_string())]),
                },
            ],
        },
        Step::Finish {
            thought: "both done".to_string(),
            answer: "parallel works".to_string(),
        },
    ])
    .await;

    let result = engine.run("parallel test").await.unwrap();
    assert_eq!(result, "parallel works");
}

#[tokio::test]
async fn unknown_tool_produces_error_observation() {
    let mut engine = build_engine(vec![
        Step::Act {
            thought: "try a bad tool".to_string(),
            calls: vec![ToolCall {
                tool: "nonexistent".to_string(),
                args: HashMap::new(),
            }],
        },
        Step::Finish {
            thought: "that failed, but I'm done".to_string(),
            answer: "handled".to_string(),
        },
    ])
    .await;

    let result = engine.run("bad tool test").await.unwrap();
    assert_eq!(result, "handled");
}

#[tokio::test]
async fn max_iterations_enforced() {
    let steps: Vec<Step> = (0..25)
        .map(|i| Step::Act {
            thought: format!("iteration {}", i),
            calls: vec![ToolCall {
                tool: "shell".to_string(),
                args: HashMap::from([("command".to_string(), "echo loop".to_string())]),
            }],
        })
        .collect();

    let mut engine = build_engine(steps).await;

    let result = engine.run("infinite loop").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("max iterations"));
}

#[tokio::test]
async fn swap_thinker_at_runtime() {
    // Start with a thinker that does one action
    let mut engine = build_engine(vec![Step::Finish {
        thought: "first brain".to_string(),
        answer: "answer from brain 1".to_string(),
    }])
    .await;

    let result = engine.run("task 1").await.unwrap();
    assert_eq!(result, "answer from brain 1");

    // Swap to a different thinker
    let new_thinker: Box<dyn Thinker> = Box::new(MockThinker::new(wrap(vec![Step::Finish {
        thought: "second brain".to_string(),
        answer: "answer from brain 2".to_string(),
    }])));
    engine.set_thinker(new_thinker).await;

    let result = engine.run("task 2").await.unwrap();
    assert_eq!(result, "answer from brain 2");
}

#[tokio::test]
async fn session_usage_accumulates_across_runs() {
    use golem::thinker::TokenUsage;

    let steps = vec![
        StepResult {
            step: Step::Finish {
                thought: "first".to_string(),
                answer: "a".to_string(),
            },
            usage: Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
            }),
        },
        StepResult {
            step: Step::Finish {
                thought: "second".to_string(),
                answer: "b".to_string(),
            },
            usage: Some(TokenUsage {
                input_tokens: 200,
                output_tokens: 75,
            }),
        },
    ];

    let thinker = Box::new(MockThinker::new(steps));
    let tools = Arc::new(ToolRegistry::new());
    tools
        .register(Arc::new(ShellTool::new(ShellConfig {
            mode: ShellMode::ReadWrite,
            working_dir: std::env::current_dir().unwrap(),
            require_confirmation: false,
            ..ShellConfig::default()
        })))
        .await;
    let memory = Box::new(SqliteMemory::in_memory().unwrap());
    let mut engine = ReactEngine::new(thinker, tools, memory, ReactConfig::default());

    engine.run("first task").await.unwrap();
    engine.run("second task").await.unwrap();

    let usage = engine.session_usage();
    assert_eq!(usage.input_tokens, 300);
    assert_eq!(usage.output_tokens, 125);
    assert_eq!(usage.total(), 425);
}

#[tokio::test]
async fn session_usage_zero_when_no_tokens() {
    let mut engine = build_engine(vec![Step::Finish {
        thought: "done".to_string(),
        answer: "ok".to_string(),
    }])
    .await;

    engine.run("task").await.unwrap();

    let usage = engine.session_usage();
    assert_eq!(usage.total(), 0);
}

#[tokio::test]
async fn memory_cleared_between_runs() {
    let steps: Vec<Step> = vec![
        // First run
        Step::Finish {
            thought: "done with first".to_string(),
            answer: "first answer".to_string(),
        },
        // Second run - the mock thinker receives context, but we just finish
        Step::Finish {
            thought: "done with second".to_string(),
            answer: "second answer".to_string(),
        },
    ];

    let mut engine = build_engine(steps).await;

    engine.run("first task").await.unwrap();
    engine.run("second task").await.unwrap();

    // Memory should only contain entries from the second run
    let history = engine.history().await.unwrap();
    assert_eq!(history.len(), 2); // Task + Answer
    assert!(
        matches!(&history[0], golem::memory::MemoryEntry::Task { content } if content == "second task")
    );
}

// ── Session memory ────────────────────────────────────────────────

#[tokio::test]
async fn session_entry_stored_after_task() {
    let thinker = Box::new(MockThinker::new(wrap(vec![Step::Finish {
        thought: "done".to_string(),
        answer: "42".to_string(),
    }])));
    let tools = Arc::new(ToolRegistry::new());
    let mem = Box::new(SqliteMemory::in_memory().unwrap());
    let mut engine = ReactEngine::new(thinker, tools, mem, ReactConfig::default());

    let history_before = engine.session_history().await.unwrap();
    assert!(history_before.is_empty());

    engine.run("what is 6 * 7").await.unwrap();

    let history_after = engine.session_history().await.unwrap();
    assert_eq!(history_after.len(), 1);
    assert_eq!(history_after[0].task, "what is 6 * 7");
    assert_eq!(history_after[0].answer, "42");
}

#[tokio::test]
async fn multi_task_session_builds_history() {
    let steps = wrap(vec![
        Step::Finish {
            thought: "first done".to_string(),
            answer: "files: a.txt, b.txt".to_string(),
        },
        Step::Finish {
            thought: "second done".to_string(),
            answer: "deleted b.txt".to_string(),
        },
    ]);

    let thinker = Box::new(MockThinker::new(steps));
    let tools = Arc::new(ToolRegistry::new());
    let memory = Box::new(SqliteMemory::in_memory().unwrap());
    let mut engine = ReactEngine::new(thinker, tools, memory, ReactConfig::default());

    engine.run("list files").await.unwrap();
    engine.run("delete the biggest one").await.unwrap();

    let history = engine.session_history().await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].task, "list files");
    assert_eq!(history[0].answer, "files: a.txt, b.txt");
    assert_eq!(history[1].task, "delete the biggest one");
    assert_eq!(history[1].answer, "deleted b.txt");
}

#[tokio::test]
async fn clear_session_resets_history() {
    let steps = wrap(vec![
        Step::Finish {
            thought: "done".to_string(),
            answer: "first answer".to_string(),
        },
        Step::Finish {
            thought: "done".to_string(),
            answer: "second answer".to_string(),
        },
    ]);

    let thinker = Box::new(MockThinker::new(steps));
    let tools = Arc::new(ToolRegistry::new());
    let memory = Box::new(SqliteMemory::in_memory().unwrap());
    let mut engine = ReactEngine::new(thinker, tools, memory, ReactConfig::default());

    engine.run("first task").await.unwrap();
    let history = engine.session_history().await.unwrap();
    assert_eq!(history.len(), 1);

    engine.clear_session().await.unwrap();
    let history = engine.session_history().await.unwrap();
    assert!(history.is_empty());

    engine.run("second task").await.unwrap();
    let history = engine.session_history().await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].task, "second task");
}

// ── Model management ──────────────────────────────────────────────

#[tokio::test]
async fn engine_model_returns_mock_default() {
    let engine = build_engine(vec![Step::Finish {
        thought: "done".to_string(),
        answer: "ok".to_string(),
    }])
    .await;

    assert_eq!(engine.model().await, "mock");
}

#[tokio::test]
async fn engine_set_model_updates_thinker() {
    let engine = build_engine(vec![Step::Finish {
        thought: "done".to_string(),
        answer: "ok".to_string(),
    }])
    .await;

    engine.set_model("claude-opus-4-20250514".to_string()).await;
    // MockThinker.set_model is a no-op, so model() still returns "mock".
    // To properly test set_model, we need a thinker that stores the model.
    // AnthropicThinker does, but requires auth. So we test the engine
    // method compiles and doesn't panic — integration tests cover the rest.
}

#[tokio::test]
async fn engine_models_returns_empty_for_mock() {
    let engine = build_engine(vec![Step::Finish {
        thought: "done".to_string(),
        answer: "ok".to_string(),
    }])
    .await;

    let models = engine.models().await.unwrap();
    assert!(models.is_empty());
}

// ── Config persistence of model preference ────────────────────────

#[test]
fn config_model_round_trip() {
    let config = golem::config::Config::open(":memory:").unwrap();

    // No model set initially
    assert!(config.get("model").unwrap().is_none());

    // Set model via /model flow
    config.set("model", "claude-opus-4-20250514").unwrap();
    assert_eq!(
        config.get("model").unwrap().unwrap(),
        "claude-opus-4-20250514"
    );

    // Change model
    config.set("model", "claude-haiku-3-20240307").unwrap();
    assert_eq!(
        config.get("model").unwrap().unwrap(),
        "claude-haiku-3-20240307"
    );
}

#[test]
fn config_model_persists_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-persist.db");
    let path_str = path.to_str().unwrap();

    // Simulate /model saving preference
    {
        let config = golem::config::Config::open(path_str).unwrap();
        config.set("model", "claude-opus-4-20250514").unwrap();
    }

    // Simulate startup reading preference
    {
        let config = golem::config::Config::open(path_str).unwrap();
        let model = config.get("model").unwrap();
        assert_eq!(model.unwrap(), "claude-opus-4-20250514");
    }
}
