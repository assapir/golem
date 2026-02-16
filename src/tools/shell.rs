use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::process::Command;

use super::Tool;

/// Executes shell commands. The agent's hands.
pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Args: {\"command\": \"<shell command>\"}"
    }

    async fn execute(&self, args: &HashMap<String, String>) -> Result<String> {
        let cmd = args.get("command").ok_or_else(|| {
            anyhow::anyhow!("missing required arg: command")
        })?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(stdout.to_string())
        } else {
            bail!(
                "exit code {}\nstdout: {}\nstderr: {}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            )
        }
    }
}
