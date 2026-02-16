use golem::memory::sqlite::SqliteMemory;
use golem::memory::{Memory, MemoryEntry};

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
