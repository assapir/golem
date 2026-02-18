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
