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

Pre-built binaries for x86_64 and aarch64 (Linux + macOS):

```bash
# Download latest release (example for x86_64 Linux)
curl -LO https://github.com/assapir/golem/releases/latest/download/golem-x86_64-linux
chmod +x golem-x86_64-linux
sudo mv golem-x86_64-linux /usr/local/bin/golem
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

## Design

Everything is a trait. Everything is swappable.

- **`Engine`** — the outermost boundary (`fn run(task) -> answer`)
- **`Thinker`** — the brain (human, Anthropic, mock — picked via `--provider`)
- **`Tool`** — something the agent can do (shell commands, more coming)
- **`Memory`** — what the agent remembers (SQLite-backed)

See [AGENTS.md](AGENTS.md) for full architecture and contributing instructions.

## Status

- [x] ReAct loop with parallel tool execution
- [x] Shell tool with security controls (read-only default, denylist, env filtering)
- [x] SQLite persistent memory
- [x] Human-in-the-loop thinker
- [x] Anthropic provider (OAuth + API key)
- [x] CI/CD with auto versioning and multi-platform releases
- [x] AUR package (`golem-bin`)
- [x] Clean Ctrl+C / Ctrl+D signal handling
- [ ] OpenAI provider
- [ ] Google Gemini provider
- [ ] Model selection
- [ ] More tools (file read/write, search)
- [ ] Self-modifying tools (runtime tool creation)

## License

GPL-2.0
