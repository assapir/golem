use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::process::Command;

use super::Tool;

/// Maximum output size in bytes. Anything beyond this is truncated.
const MAX_OUTPUT_BYTES: usize = 50_000;

/// Commands that are never allowed regardless of mode.
const BLOCKED_COMMANDS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "mkfs",
    "dd if=",
    ":(){ :|:& };:",
    "> /dev/sda",
    "chmod -R 777 /",
    "fork bomb",
    "shutdown",
    "reboot",
    "halt",
    "init 0",
    "init 6",
];

/// Commands/patterns that require write mode.
const WRITE_PATTERNS: &[&str] = &[
    "rm ",
    "rmdir",
    "mv ",
    "cp ",
    "mkdir",
    "touch ",
    "chmod",
    "chown",
    "chgrp",
    "ln ",
    "install ",
    "dd ",
    "mkfs",
    "fdisk",
    "parted",
    "mount",
    "umount",
    "kill",
    "killall",
    "pkill",
    "systemctl start",
    "systemctl stop",
    "systemctl restart",
    "systemctl enable",
    "systemctl disable",
    "docker rm",
    "docker stop",
    "docker kill",
    "apt ",
    "yay ",
    "pacman -S",
    "pacman -R",
    "pip install",
    "cargo install",
    "npm install",
    "git push",
    "git commit",
    "git reset",
    "git checkout",
    "git merge",
    "git rebase",
    "curl.*-X POST",
    "curl.*-X PUT",
    "curl.*-X DELETE",
    "wget ",
    "> ",
    ">> ",
    "tee ",
    "sed -i",
    "truncate",
];

/// Safe environment variables to pass through. Everything else is stripped.
const SAFE_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TERM",
    "TZ",
];

/// Shell execution mode.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellMode {
    /// Only read-only commands allowed (default).
    ReadOnly,
    /// All commands allowed (except always-blocked ones).
    ReadWrite,
}

/// Configuration for the shell tool.
#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub mode: ShellMode,
    pub working_dir: PathBuf,
    pub max_output_bytes: usize,
    pub require_confirmation: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            mode: ShellMode::ReadOnly,
            working_dir: std::env::temp_dir().join("golem-sandbox"),
            max_output_bytes: MAX_OUTPUT_BYTES,
            require_confirmation: true,
        }
    }
}

/// Executes shell commands with safety controls.
pub struct ShellTool {
    config: ShellConfig,
}

impl ShellTool {
    pub fn new(config: ShellConfig) -> Self {
        Self { config }
    }

    /// Check if a command is always blocked.
    fn is_blocked(cmd: &str) -> bool {
        let lower = cmd.to_lowercase();
        BLOCKED_COMMANDS.iter().any(|pat| lower.contains(pat))
    }

    /// Check if a command requires write mode.
    fn is_write_command(cmd: &str) -> bool {
        let trimmed = cmd.trim();

        // Pipe chains: check each segment
        for segment in trimmed.split('|') {
            let seg = segment.trim();
            if Self::segment_is_write(seg) {
                return true;
            }
        }

        // Command chains: ;, &&, ||
        for segment in trimmed.split(&[';', '&', '|'][..]) {
            let seg = segment.trim();
            if Self::segment_is_write(seg) {
                return true;
            }
        }

        false
    }

    fn segment_is_write(segment: &str) -> bool {
        let seg = segment.trim();
        if seg.is_empty() {
            return false;
        }

        // Check for output redirection
        if seg.contains("> ") || seg.contains(">>") {
            return true;
        }

        WRITE_PATTERNS.iter().any(|pat| {
            // Check if pattern matches the start of the command or appears after sudo
            let seg_lower = seg.to_lowercase();
            let pat_lower = pat.to_lowercase();
            seg_lower.starts_with(&pat_lower)
                || seg_lower.starts_with(&format!("sudo {}", pat_lower))
                || seg_lower.contains(&pat_lower)
        })
    }

    fn truncate_output(output: &str, max_bytes: usize) -> String {
        if output.len() <= max_bytes {
            return output.to_string();
        }
        let truncated = &output[..max_bytes];
        // Find last valid UTF-8 boundary
        let truncated = match truncated.char_indices().last() {
            Some((i, c)) => &truncated[..i + c.len_utf8()],
            None => truncated,
        };
        format!(
            "{}\n\n[truncated: showing {}/{} bytes]",
            truncated,
            max_bytes,
            output.len()
        )
    }

    fn filtered_env() -> Vec<(String, String)> {
        SAFE_ENV_VARS
            .iter()
            .filter_map(|key| {
                std::env::var(key).ok().map(|val| (key.to_string(), val))
            })
            .collect()
    }

    fn confirm(cmd: &str) -> Result<bool> {
        print!("  Execute: {} [y/N] ", cmd);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().eq_ignore_ascii_case("y"))
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        match self.config.mode {
            ShellMode::ReadOnly => {
                "Execute a read-only shell command. Args: {\"command\": \"<shell command>\"}. Write operations are blocked."
            }
            ShellMode::ReadWrite => {
                "Execute a shell command. Args: {\"command\": \"<shell command>\"}. Write operations are allowed."
            }
        }
    }

    async fn execute(&self, args: &HashMap<String, String>) -> Result<String> {
        let cmd = args
            .get("command")
            .ok_or_else(|| anyhow::anyhow!("missing required arg: command"))?;

        // Always block dangerous commands
        if Self::is_blocked(cmd) {
            bail!("blocked: command is on the deny list");
        }

        // Check write mode
        if self.config.mode == ShellMode::ReadOnly && Self::is_write_command(cmd) {
            bail!(
                "blocked: write operation not allowed in read-only mode. \
                 Start golem with --allow-write to enable write operations."
            );
        }

        // Confirmation prompt
        if self.config.require_confirmation
            && !Self::confirm(cmd)?
        {
            bail!("cancelled by user");
        }

        // Ensure working directory exists
        let work_dir = &self.config.working_dir;
        if !work_dir.exists() {
            tokio::fs::create_dir_all(work_dir).await?;
        }

        // Build command with sanitized environment
        let env_vars = Self::filtered_env();
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(work_dir)
            .env_clear()
            .envs(env_vars)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(Self::truncate_output(&stdout, self.config.max_output_bytes))
        } else {
            bail!(
                "exit code {}\nstdout: {}\nstderr: {}",
                output.status.code().unwrap_or(-1),
                Self::truncate_output(&stdout, self.config.max_output_bytes),
                Self::truncate_output(&stderr, self.config.max_output_bytes)
            )
        }
    }
}
