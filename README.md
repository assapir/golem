# Golem

A clay body, animated by words.

Golem is a minimal AI agent harness built in Rust. It has no model of its own — it borrows its intelligence from whatever you plug in: a human at a keyboard, a local LLM, or a cloud API.

Built as a learning project to explore ReAct, tool calling, and memory from scratch.

## What it does

```
You give a task → Thinker reasons → Tools execute → Observations fed back → Repeat
```

This is the [ReAct](https://arxiv.org/abs/2210.03629) pattern: **Re**ason + **Act** in a loop until the task is done.

## Quick start

```bash
cargo build
cargo run

# golem v0.1.0 — a clay body, animated by words
#
# golem> list files in the current directory
# Thought: ...
# Action: shell:ls -la
# [shell] ✓ ...
```

## Design

Everything is a trait. Everything is swappable.

- **`Engine`** — the outermost boundary (`fn run(task) -> answer`)
- **`Thinker`** — the brain (human, LLM, mock — picked via `--provider`)
- **`Tool`** — something the agent can do (shell commands, more coming)
- **`Memory`** — what the agent remembers (SQLite-backed)

See [AGENTS.md](AGENTS.md) for full architecture and contributing instructions.

## CLI

```
Usage: golem [OPTIONS]

Options:
  -p, --provider <PROVIDER>    LLM provider [default: human] [possible values: human]
      --model <MODEL>          Model name (provider-specific)
  -d, --db <DB>                SQLite database path [default: golem.db]
  -m, --max-iterations <N>     Max ReAct loop iterations [default: 20]
  -t, --timeout <SECONDS>      Tool execution timeout [default: 30]
  -r, --run <TASK>             Run a single task and exit
  -h, --help                   Print help
  -V, --version                Print version
```

## Status

This is a learning project. Currently implemented:

- [x] ReAct loop
- [x] Shell tool (execute terminal commands)
- [x] SQLite persistent memory
- [x] Human-in-the-loop thinker (you are the LLM)
- [x] Parallel tool execution
- [x] Runtime thinker swapping
- [ ] LLM provider (Ollama)
- [ ] More tools (file read/write, search)
- [ ] Self-modifying tools (runtime tool creation)
- [ ] Auth / middleware

## License

GPL-2.0
