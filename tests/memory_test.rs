use golem::memory::sqlite::SqliteMemory;
use golem::memory::{Memory, MemoryEntry, SessionEntry};
use golem::tools::{Outcome, ToolResult};

#[tokio::test]
async fn store_and_retrieve_history() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store(MemoryEntry::Task {
        content: "test task".to_string(),
    })
    .await
    .unwrap();

    let history = mem.history().await.unwrap();
    assert_eq!(history.len(), 1);
    assert!(matches!(&history[0], MemoryEntry::Task { content } if content == "test task"));
}

#[tokio::test]
async fn history_preserves_order() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store(MemoryEntry::Task {
        content: "first".to_string(),
    })
    .await
    .unwrap();

    mem.store(MemoryEntry::Answer {
        thought: "done".to_string(),
        content: "second".to_string(),
    })
    .await
    .unwrap();

    let history = mem.history().await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(matches!(&history[0], MemoryEntry::Task { .. }));
    assert!(matches!(&history[1], MemoryEntry::Answer { .. }));
}

#[tokio::test]
async fn recall_finds_matching_entries() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store(MemoryEntry::Task {
        content: "find the needle".to_string(),
    })
    .await
    .unwrap();

    mem.store(MemoryEntry::Task {
        content: "something else".to_string(),
    })
    .await
    .unwrap();

    let results = mem.recall("needle").await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn clear_removes_all_entries() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store(MemoryEntry::Task {
        content: "task".to_string(),
    })
    .await
    .unwrap();

    mem.clear().await.unwrap();

    let history = mem.history().await.unwrap();
    assert!(history.is_empty());
}

#[test]
fn display_task() {
    let entry = MemoryEntry::Task {
        content: "do something".to_string(),
    };
    assert_eq!(format!("{}", entry), "Task: do something");
}

#[test]
fn display_answer() {
    let entry = MemoryEntry::Answer {
        thought: "figured it out".to_string(),
        content: "42".to_string(),
    };
    assert_eq!(format!("{}", entry), "Answer (figured it out): 42");
}

#[test]
fn display_iteration_with_success_and_error() {
    let entry = MemoryEntry::Iteration {
        thought: "trying two things".to_string(),
        results: vec![
            ToolResult {
                tool: "shell".to_string(),
                outcome: Outcome::Success("hello world".to_string()),
            },
            ToolResult {
                tool: "shell".to_string(),
                outcome: Outcome::Error("not found".to_string()),
            },
        ],
    };
    let display = format!("{}", entry);
    assert!(display.contains("Thought: trying two things"));
    assert!(display.contains("[shell] ✓ hello world"));
    assert!(display.contains("[shell] ✗ not found"));
}

#[test]
fn display_iteration_truncates_long_output() {
    let long_output = "x".repeat(500);
    let entry = MemoryEntry::Iteration {
        thought: "checking".to_string(),
        results: vec![ToolResult {
            tool: "shell".to_string(),
            outcome: Outcome::Success(long_output),
        }],
    };
    let display = format!("{}", entry);
    // Should be truncated to 200 chars
    let success_line = display.lines().nth(1).unwrap();
    // "[shell] ✓ " prefix + 200 chars of 'x'
    assert!(success_line.contains(&"x".repeat(200)));
    assert!(!success_line.contains(&"x".repeat(201)));
}

// ── Session memory ────────────────────────────────────────────────

#[tokio::test]
async fn session_store_and_retrieve() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store_session(SessionEntry {
        task: "list files".to_string(),
        answer: "file1.txt, file2.txt".to_string(),
    })
    .await
    .unwrap();

    let history = mem.session_history(50).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].task, "list files");
    assert_eq!(history[0].answer, "file1.txt, file2.txt");
}

#[tokio::test]
async fn session_history_preserves_order() {
    let mem = SqliteMemory::in_memory().unwrap();

    for i in 1..=3 {
        mem.store_session(SessionEntry {
            task: format!("task {i}"),
            answer: format!("answer {i}"),
        })
        .await
        .unwrap();
    }

    let history = mem.session_history(50).await.unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].task, "task 1");
    assert_eq!(history[1].task, "task 2");
    assert_eq!(history[2].task, "task 3");
}

#[tokio::test]
async fn session_history_respects_limit() {
    let mem = SqliteMemory::in_memory().unwrap();

    for i in 1..=10 {
        mem.store_session(SessionEntry {
            task: format!("task {i}"),
            answer: format!("answer {i}"),
        })
        .await
        .unwrap();
    }

    let history = mem.session_history(3).await.unwrap();
    assert_eq!(history.len(), 3);
    // Should be the LAST 3, in order
    assert_eq!(history[0].task, "task 8");
    assert_eq!(history[1].task, "task 9");
    assert_eq!(history[2].task, "task 10");
}

#[tokio::test]
async fn session_clear_removes_all() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store_session(SessionEntry {
        task: "task".to_string(),
        answer: "answer".to_string(),
    })
    .await
    .unwrap();

    mem.clear_session().await.unwrap();

    let history = mem.session_history(50).await.unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn session_clear_does_not_affect_task_memory() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store(MemoryEntry::Task {
        content: "current task".to_string(),
    })
    .await
    .unwrap();

    mem.store_session(SessionEntry {
        task: "old task".to_string(),
        answer: "old answer".to_string(),
    })
    .await
    .unwrap();

    mem.clear_session().await.unwrap();

    // Task memory should be untouched
    let history = mem.history().await.unwrap();
    assert_eq!(history.len(), 1);

    // Session memory should be cleared
    let session = mem.session_history(50).await.unwrap();
    assert!(session.is_empty());
}

#[tokio::test]
async fn task_clear_does_not_affect_session_memory() {
    let mem = SqliteMemory::in_memory().unwrap();

    mem.store_session(SessionEntry {
        task: "prior task".to_string(),
        answer: "prior answer".to_string(),
    })
    .await
    .unwrap();

    mem.store(MemoryEntry::Task {
        content: "current task".to_string(),
    })
    .await
    .unwrap();

    mem.clear().await.unwrap();

    // Task memory should be cleared
    let history = mem.history().await.unwrap();
    assert!(history.is_empty());

    // Session memory should be untouched
    let session = mem.session_history(50).await.unwrap();
    assert_eq!(session.len(), 1);
    assert_eq!(session[0].task, "prior task");
}

#[tokio::test]
async fn session_persists_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-test.db");
    let path_str = path.to_str().unwrap();

    // Store session entry
    {
        let mem = SqliteMemory::new(path_str).unwrap();
        mem.store_session(SessionEntry {
            task: "persisted task".to_string(),
            answer: "persisted answer".to_string(),
        })
        .await
        .unwrap();
    }

    // Reopen and verify
    {
        let mem = SqliteMemory::new(path_str).unwrap();
        let history = mem.session_history(50).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].task, "persisted task");
    }
}
