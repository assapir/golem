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
| Provider wiring | `src/provider.rs` |

## Code style

- All public traits must be `Send + Sync` (async + parallel execution).
- Tool errors become `Outcome::Error(String)`, never panics or propagated `Result::Err`.
- Use `env!("CARGO_PKG_VERSION")` and `env!("CARGO_PKG_*")` — never hardcode metadata.
- `main.rs` imports from the library crate (`use golem::...`), not `mod` declarations.
- Project constants go in `src/consts.rs`, display logic in `src/banner.rs`.
- Default model constants belong in the thinker module that uses them, not in `consts.rs`.

## Module layout

```
src/
├── main.rs              # CLI parsing + REPL loop (no provider logic)
├── lib.rs               # re-exports
├── provider.rs          # ProviderConfig trait, provider impls, build/login/logout
├── banner.rs            # startup banner + session summary
├── commands/            # Command trait + CommandRegistry + built-in /slash commands
├── config/              # SQLite key-value config (model preference, etc.)
├── consts.rs            # project-wide constants (from Cargo.toml metadata)
├── auth/                # OAuth PKCE flows + credential storage (SQLite)
│   ├── mod.rs           # login/logout routing
│   ├── oauth.rs         # Anthropic OAuth PKCE
│   ├── google_oauth.rs  # Google OAuth PKCE (loopback redirect)
│   └── storage.rs       # AuthStorage (SQLite credentials table)
├── engine/              # Engine trait + ReactEngine (ReAct loop)
├── events.rs            # EventBus (tokio broadcast) for decoupled communication
├── prompts/             # shared ReAct system prompt builder
├── thinker/             # Thinker trait + providers
│   ├── mod.rs           # Thinker trait, Step, parse_response
│   ├── anthropic.rs     # Anthropic Messages API
│   ├── gemini.rs        # Google Gemini generateContent API
│   ├── human.rs         # Human-in-the-loop thinker
│   └── mock.rs          # MockThinker for tests
├── tools/               # Tool trait + ToolRegistry + ShellTool
├── memory/              # Memory trait + SqliteMemory (task + session memory)
└── spinner.rs           # Thinking spinner during LLM calls
```

## Adding a new provider

1. Create `src/thinker/my_provider.rs`, implement `Thinker` trait:
   - Use `build_react_system_prompt()` from `src/prompts/react.rs` — don't duplicate.
   - Return `StepResult { step, usage: Option<TokenUsage> }` from `next_step()`.
   - Implement `models()`, `model()`, `set_model()` for model selection support.
   - Own the default model as a `const` in the module.
2. If the provider needs OAuth, create `src/auth/my_oauth.rs` with the flow.
3. In `src/provider.rs`:
   - Add a module implementing `ProviderConfig` (5 methods: `id`, `display_name`, `env_var`, `build_thinker`, `login`).
   - Add variants to `Provider` and `LoginProvider` enums.
   - Add a match arm in `Provider::config()`, `LoginProvider::config()`, and `provider_config_by_id()`.
4. Register the thinker module in `src/thinker/mod.rs`.
5. Add the provider to `SUPPORTED_PROVIDERS` in `src/auth/mod.rs` if it has OAuth.
6. Test with `MockThinker` in `tests/react_test.rs`.

## Adding a new tool

1. Create `src/tools/my_tool.rs`, implement `Tool` trait (`Send + Sync + async`).
2. Register in `main.rs`: `tools.register(Arc::new(MyTool)).await;`
3. Add tests in `tests/tools_test.rs`.

## Adding a new command

1. Create `src/commands/my_cmd.rs`, implement `Command` trait (`Send + Sync + async`).
2. Register in `CommandRegistry::new()` in `src/commands/mod.rs`.
3. Return `CommandResult::Handled`, `StateChanged(StateChange::*)`, or `Quit`.
4. Add tests in the command file's `#[cfg(test)]` module.

## Key abstractions

- **`ProviderConfig`** — trait defining a provider's identity, auth, and thinker construction. Each provider (Anthropic, Google) implements it. The `Provider` and `LoginProvider` CLI enums dispatch to implementations via `config()`.
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
