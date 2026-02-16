use std::collections::HashMap;
use std::sync::Arc;

use golem::tools::shell::{ShellConfig, ShellMode, ShellTool};
use golem::tools::{Outcome, ToolRegistry};

/// Helper: build a shell tool with no confirmation, read-write mode, cwd as work dir.
fn test_shell() -> ShellTool {
    ShellTool::new(ShellConfig {
        mode: ShellMode::ReadWrite,
        working_dir: std::env::current_dir().unwrap(),
        require_confirmation: false,
        ..ShellConfig::default()
    })
}

/// Helper: build a read-only shell tool with no confirmation.
fn readonly_shell() -> ShellTool {
    ShellTool::new(ShellConfig {
        mode: ShellMode::ReadOnly,
        working_dir: std::env::current_dir().unwrap(),
        require_confirmation: false,
        ..ShellConfig::default()
    })
}

#[tokio::test]
async fn shell_tool_executes_command() {
    let tool = test_shell();
    let args = HashMap::from([("command".to_string(), "echo hello".to_string())]);

    let result = golem::tools::Tool::execute(&tool, &args).await.unwrap();
    assert_eq!(result.trim(), "hello");
}

#[tokio::test]
async fn shell_tool_returns_error_on_bad_command() {
    let tool = test_shell();
    let args = HashMap::from([("command".to_string(), "false".to_string())]);

    let result = golem::tools::Tool::execute(&tool, &args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn shell_tool_requires_command_arg() {
    let tool = test_shell();
    let args = HashMap::new();

    let result = golem::tools::Tool::execute(&tool, &args).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("command"));
}

#[tokio::test]
async fn shell_tool_blocks_dangerous_commands() {
    let tool = test_shell();
    let args = HashMap::from([("command".to_string(), "rm -rf /".to_string())]);

    let result = golem::tools::Tool::execute(&tool, &args).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blocked"));
}

#[tokio::test]
async fn shell_readonly_blocks_write_commands() {
    let tool = readonly_shell();

    let write_commands = vec![
        "rm file.txt",
        "mv a b",
        "cp a b",
        "mkdir /tmp/test",
        "touch file.txt",
        "echo hello > file.txt",
        "echo hello >> file.txt",
        "sed -i 's/a/b/' file.txt",
        "git push origin main",
        "kill 1234",
    ];

    for cmd in write_commands {
        let args = HashMap::from([("command".to_string(), cmd.to_string())]);
        let result = golem::tools::Tool::execute(&tool, &args).await;
        assert!(
            result.is_err(),
            "expected '{}' to be blocked in read-only mode, but it succeeded",
            cmd
        );
        assert!(
            result.unwrap_err().to_string().contains("read-only"),
            "expected read-only error for '{}'",
            cmd
        );
    }
}

#[tokio::test]
async fn shell_readonly_allows_read_commands() {
    let tool = readonly_shell();

    let read_commands = vec![
        "echo hello",
        "cat /etc/hostname",
        "ls /tmp",
        "pwd",
        "whoami",
        "uname -a",
        "date",
        "df -h",
        "ps aux",
        "env",
    ];

    for cmd in read_commands {
        let args = HashMap::from([("command".to_string(), cmd.to_string())]);
        let result = golem::tools::Tool::execute(&tool, &args).await;
        assert!(
            result.is_ok(),
            "expected '{}' to be allowed in read-only mode, but got: {}",
            cmd,
            result.unwrap_err()
        );
    }
}

#[tokio::test]
async fn shell_readwrite_allows_write_commands() {
    let tool = test_shell();
    // mkdir in a temp dir should work in read-write mode
    let dir = std::env::temp_dir().join("golem-test-rw");
    let args = HashMap::from([(
        "command".to_string(),
        format!("mkdir -p {} && rmdir {}", dir.display(), dir.display()),
    )]);

    let result = golem::tools::Tool::execute(&tool, &args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn shell_truncates_large_output() {
    let tool = ShellTool::new(ShellConfig {
        mode: ShellMode::ReadOnly,
        working_dir: std::env::current_dir().unwrap(),
        require_confirmation: false,
        max_output_bytes: 100,
    });

    // Generate output larger than 100 bytes
    let args = HashMap::from([("command".to_string(), "seq 1 1000".to_string())]);
    let result = golem::tools::Tool::execute(&tool, &args).await.unwrap();

    assert!(result.contains("[truncated:"));
}

#[tokio::test]
async fn shell_filters_environment() {
    let tool = readonly_shell();
    // CARGO_PKG_NAME is set by cargo during tests but is NOT in our safe list.
    // It should be filtered out by env_clear().
    let args = HashMap::from([("command".to_string(), "env".to_string())]);
    let result = golem::tools::Tool::execute(&tool, &args).await.unwrap();

    assert!(
        !result.contains("CARGO_PKG_NAME"),
        "non-safe env var leaked! output: {}",
        result
    );
    // PATH should still be there (it's in the safe list)
    assert!(result.contains("PATH="), "PATH should be preserved");
}

#[tokio::test]
async fn registry_executes_known_tool() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(test_shell())).await;

    let args = HashMap::from([("command".to_string(), "echo works".to_string())]);
    let result = registry.execute("shell", &args).await;

    assert!(matches!(result.outcome, Outcome::Success(ref s) if s.trim() == "works"));
}

#[tokio::test]
async fn registry_returns_error_for_unknown_tool() {
    let registry = ToolRegistry::new();

    let result = registry.execute("nonexistent", &HashMap::new()).await;
    assert!(matches!(result.outcome, Outcome::Error(ref s) if s.contains("unknown tool")));
}

#[tokio::test]
async fn registry_unregister_removes_tool() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(test_shell())).await;

    assert_eq!(registry.descriptions().await.len(), 1);

    registry.unregister("shell").await;

    assert_eq!(registry.descriptions().await.len(), 0);
}
