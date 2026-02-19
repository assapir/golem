# Golem

A clay body, animated by words.

Golem is a minimal AI agent harness built in Rust. It has no model of its own — it borrows its intelligence from whatever you plug in: a human at a keyboard, a local LLM, or a cloud API.

Built as a learning project to explore ReAct, tool calling, and memory from scratch.

## What it does

```
You give a task → Thinker reasons → Tools execute → Observations fed back → Repeat
```

This is the [ReAct](https://arxiv.org/abs/2210.03629) pattern: **Re**ason + **Act** in a loop until the task is done.

## Install

### AUR (Arch Linux)

```bash
yay -S golem-bin
```

### GitHub Releases

Pre-built binaries for x86_64 and aarch64 (Linux + macOS + Windows):

```bash
# Download latest release (example for x86_64 Linux)
curl -LO https://github.com/assapir/golem/releases/latest/download/golem-x86_64-linux
chmod +x golem-x86_64-linux
sudo mv golem-x86_64-linux /usr/local/bin/golem
```

```powershell
# Windows (PowerShell)
Invoke-WebRequest -Uri https://github.com/assapir/golem/releases/latest/download/golem-x86_64-windows.exe -OutFile golem.exe
```

### Build from source

```bash
git clone https://github.com/assapir/golem.git
cd golem
cargo build --release
```

## Quick start

```bash
# Log in to Anthropic (opens browser for OAuth)
golem login

# Interactive mode
golem

# Single task
golem -r "list files in the current directory"

# golem v0.2.0 — a clay body, animated by words
#
# golem> list files in the current directory
# Thought: I need to list the files...
# Action: shell:ls -la
# [shell] ✓ ...
```

## CLI

```
Usage: golem [OPTIONS] [COMMAND]

Commands:
  login   Log in to an LLM provider via OAuth
  logout  Log out from an LLM provider
  help    Print this message or the help of the given subcommand(s)

Options:
  -p, --provider <PROVIDER>    LLM provider [default: anthropic] [possible values: human, anthropic]
      --model <MODEL>          Model name (provider-specific, ignored for human)
  -d, --db <DB>                SQLite database path [default: golem.db]
  -m, --max-iterations <N>     Max ReAct loop iterations [default: 20]
  -t, --timeout <SECONDS>      Tool execution timeout [default: 30]
      --allow-write            Allow write operations in shell (default: read-only)
  -w, --work-dir <PATH>        Working directory for shell commands
      --no-confirm             Skip confirmation prompts before executing commands
  -r, --run <TASK>             Run a single task and exit
  -h, --help                   Print help
  -V, --version                Print version
```

## REPL commands

Type `/help` at the prompt to see all available commands:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/help` | `/h`, `/?` | Show available commands |
| `/whoami` | | Show provider, model, and auth status |
| `/tools` | | List registered tools |
| `/tokens` | | Show session token usage |
| `/model` | | List and switch the active model |
| `/new` | | Start a new session (clear conversation history) |
| `/login` | | Log in to the current provider |
| `/logout` | | Log out from the current provider |
| `/quit` | `quit`, `exit`, `/exit` | Exit the REPL |

Commands are trait-based (`Command` trait + `CommandRegistry`) — plugins can register additional commands at runtime.

## Session memory

Golem remembers prior tasks within a session. Each completed task's question and answer are stored in SQLite, so follow-up tasks can reference earlier context:

```
golem> list files in /tmp
=> file1.txt (10KB), file2.txt (50KB), file3.txt (1KB)

golem> delete the biggest one
=> (LLM knows file2.txt is 50KB from the prior task)
```

Session history persists across restarts. Use `/new` to clear it and start fresh. The last 50 task summaries are kept by default.

## Design

Everything is a trait. Everything is swappable.

- **`Engine`** — the outermost boundary (`fn run(task) -> answer`)
- **`Thinker`** — the brain (human, Anthropic, mock — picked via `--provider`)
- **`Tool`** — something the agent can do (shell commands, more coming)
- **`Command`** — built-in REPL commands (`/help`, `/model`, `/new`, etc.)
- **`Memory`** — what the agent remembers (task iterations + session history, SQLite-backed)
- **`Config`** — persistent key-value settings (model preference, etc.)
- **`EventBus`** — decoupled broadcast channel for cross-component communication

See [AGENTS.md](AGENTS.md) for full architecture and contributing instructions.

## License

GPL-2.0
