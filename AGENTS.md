# Golem

Rust AI agent harness with ReAct loop, pluggable tools, and SQLite memory.

## Build and test

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo test                     # run all tests — must pass before committing
cargo clippy --all-targets     # must have zero warnings
cargo fmt                      # CI enforces cargo fmt --check
```

## Testing

- Unit tests live in `src/` (inline `#[cfg(test)]` modules).
- Integration tests live in `tests/` — one file per module.
- `MockThinker` scripts agent behavior via `Vec<StepResult>`.
- Always test both success and error paths.

| Changed | Test file |
|---------|-----------|
| ReAct loop | `tests/react_test.rs` |
| Tools | `tests/tools_test.rs` |
| Memory (task + session) | `tests/memory_test.rs` |
| Auth | `tests/auth_test.rs` |
| Config | `src/config/mod.rs` |
| Events | `src/events.rs` |
| Prompts | `src/prompts/react.rs` |
| Constants | `src/consts.rs` |
| Banner | `src/banner.rs` |

## Code style

- All public traits must be `Send + Sync` (async + parallel execution).
- Tool errors become `Outcome::Error(String)`, never panics or propagated `Result::Err`.
- Use `env!("CARGO_PKG_VERSION")` and `env!("CARGO_PKG_*")` — never hardcode metadata.
- `main.rs` imports from the library crate (`use golem::...`), not `mod` declarations.
- Project constants go in `src/consts.rs`, display logic in `src/banner.rs`.

## Module layout

```
src/
├── main.rs              # CLI, wiring, REPL
├── lib.rs               # re-exports
├── banner.rs            # startup banner + session summary
├── commands/            # Command trait + CommandRegistry + built-in /slash commands
├── config/              # SQLite key-value config (model preference, etc.)
├── consts.rs            # project-wide constants (from Cargo.toml metadata)
├── auth/                # OAuth PKCE flow + credential storage (SQLite)
├── engine/              # Engine trait + ReactEngine (ReAct loop)
├── events.rs            # EventBus (tokio broadcast) for decoupled communication
├── prompts/             # shared ReAct system prompt builder
├── thinker/             # Thinker trait + providers (anthropic, human, mock)
├── tools/               # Tool trait + ToolRegistry + ShellTool
└── memory/              # Memory trait + SqliteMemory (task + session memory)
```

## Adding a new tool

1. Create `src/tools/my_tool.rs`, implement `Tool` trait (`Send + Sync + async`).
2. Register in `main.rs`: `tools.register(Arc::new(MyTool)).await;`
3. Add tests in `tests/tools_test.rs`.

## Adding a new provider

1. Create `src/thinker/my_provider.rs`, implement `Thinker` trait.
2. Use `build_react_system_prompt()` from `src/prompts/react.rs` — don't duplicate.
3. Return `StepResult { step, usage: Option<TokenUsage> }` from `next_step()`.
4. Implement `models()`, `model()`, `set_model()` for model selection support.
5. Add `Provider` enum variant + match arm in `main.rs`.
6. Test with `MockThinker` in `tests/react_test.rs`.

## Adding a new command

1. Create `src/commands/my_cmd.rs`, implement `Command` trait (`Send + Sync + async`).
2. Register in `CommandRegistry::new()` in `src/commands/mod.rs`.
3. Return `CommandResult::Handled`, `StateChanged(StateChange::*)`, or `Quit`.
4. Add tests in the command file's `#[cfg(test)]` module.

## Key abstractions

- **`StateChange`** — enum for REPL state updates (`Auth`, `Model`). Commands return `CommandResult::StateChanged(StateChange::*)` and the REPL applies the change.
- **`EventBus`** — `tokio::sync::broadcast` channel for decoupled notifications. Components subscribe via `bus.subscribe()`.
- **`SessionEntry`** — task + answer summary persisted across tasks. Loaded into `Context.session_history` so the LLM sees prior conversation.
- **`Config`** — SQLite key-value store for persistent settings (model preference, etc.).

## Workflow

- **Always use worktrees** — never edit main directly. Branch in `.worktrees/`, push, open PR.
- **Update docs** — AGENTS.md and README.md must reflect changes before opening a PR.
- **Version is auto-managed** — don't edit `Cargo.toml` version manually. CI bumps it on release.

## CI/CD

- **CI**: `cargo fmt --check` + `cargo clippy` (warnings = errors) + `cargo test` on PRs.
- **Release**: auto version bump → 4-platform build matrix → GitHub Release → AUR publish.
- Skip version bump commits in CI via commit message prefix.
