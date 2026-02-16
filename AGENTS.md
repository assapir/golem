# Golem

A clay body, animated by words. Rust AI agent harness with ReAct loop, pluggable tools, and SQLite memory.

## Build and run

```bash
cargo build                           # debug build
cargo build --release                 # release build
cargo run                             # interactive REPL
cargo run -- --help                   # show CLI options
cargo run -- --provider human         # explicit provider (default)
cargo run -- -r "some task"           # single task, non-interactive
cargo run -- -d custom.db             # custom SQLite path
cargo run -- -d :memory:              # ephemeral memory (no persistence)
cargo run -- -m 10 -t 60             # max 10 iterations, 60s tool timeout
cargo run -- --allow-write            # enable write operations in shell
cargo run -- --no-confirm             # skip confirmation prompts
cargo run -- -w /path/to/dir          # set shell working directory
```

## Testing

- Run `cargo test` before committing any change.
- Tests live in `tests/` as integration tests. No unit tests in `src/` currently.
- `MockThinker` is used in tests to script agent behavior — add steps as `Vec<Step>`.
- When adding a new tool, add tests in `tests/tools_test.rs`.
- When changing the ReAct loop, add tests in `tests/react_test.rs`.
- When changing memory, add tests in `tests/memory_test.rs`.
- Always test both success and error paths. Errors are observations, not crashes.

```bash
cargo test                            # run all tests
cargo test react                      # run react loop tests only
cargo test tools                      # run tool tests only
cargo test memory                     # run memory tests only
```

## Code style

- Run `cargo clippy` and fix all warnings before committing.
- All public traits must be `Send + Sync` — this is required for async + parallel tool execution.
- Use `Arc` for shared ownership across async boundaries, `RwLock` for mutable shared state.
- Errors from tool execution are **never** panics or `Result::Err` propagated to the loop. They become `Outcome::Error(String)` and get fed back to the thinker as observations.
- Prefer `async` functions. Tools, memory, and thinkers are all async traits.
- Never hardcode version strings — use `env!("CARGO_PKG_VERSION")`.
- Never write files that can be generated — use `cargo init`, `cargo add`, etc.

## Architecture

Trait-based dependency injection. Everything is swappable.

```
main.rs → Box<dyn Engine>
               │
         ReactEngine
          ├── Arc<RwLock<Box<dyn Thinker>>>   # the brain (human, LLM, mock)
          ├── Arc<ToolRegistry>                # RwLock<HashMap<String, Arc<dyn Tool>>>
          └── Box<dyn Memory>                  # SQLite, in-memory
```

### Key traits

| Trait | Purpose | Implementations |
|-------|---------|-----------------|
| `Engine` | Outermost boundary, `fn run(task) -> answer` | `ReactEngine` |
| `Thinker` | Produces next `Step` given context | `HumanThinker`, `MockThinker` |
| `Tool` | Executes an action, returns string | `ShellTool` |
| `Memory` | Stores and retrieves `MemoryEntry` | `SqliteMemory` |

### Module layout

```
src/
├── main.rs              # CLI, wiring, REPL
├── lib.rs               # re-exports for integration tests
├── engine/
│   ├── mod.rs           # Engine trait
│   └── react.rs         # ReactEngine (ReAct loop)
├── thinker/
│   ├── mod.rs           # Thinker trait, Step, ToolCall, Context
│   ├── human.rs         # interactive stdin thinker
│   └── mock.rs          # scripted thinker for tests
├── tools/
│   ├── mod.rs           # Tool trait, ToolRegistry, ToolResult, Outcome
│   └── shell.rs         # shell command execution
└── memory/
    ├── mod.rs           # Memory trait, MemoryEntry
    └── sqlite.rs        # SQLite-backed persistent memory
```

## Design decisions

- **`Step::Act` always has `Vec<ToolCall>`** — a single call is `vec![one]`, no separate parallel variant.
- **`ToolResult` contains `Outcome::Success` or `Outcome::Error`** — errors are information, not failures. The thinker decides what to do.
- **The ReAct loop only enforces hard limits** — max iterations and tool timeout. All intelligence is in the thinker.
- **Thinker is swappable at runtime** via `engine.set_thinker()`. The `Arc<RwLock<...>>` wrapper makes this safe between iterations.
- **Provider ≠ Model** — `--provider` selects the API (human, ollama, anthropic), `--model` selects which model on that provider.
- **ToolRegistry uses `RwLock`** — supports runtime tool registration/unregistration for self-modification.

## Adding a new tool

1. Create `src/tools/my_tool.rs`, implement the `Tool` trait.
2. Register it in `main.rs`: `tools.register(Arc::new(MyTool)).await;`
3. Add tests in `tests/tools_test.rs`.
4. The tool must be `Send + Sync` and its `execute` must be `async`.

## Adding a new thinker (provider)

1. Create `src/thinker/my_provider.rs`, implement the `Thinker` trait.
2. Add a variant to the `Provider` enum in `main.rs`.
3. Add the match arm to construct the thinker.
4. Test with `MockThinker` patterns in `tests/react_test.rs`.

## Security model

The shell tool is locked down by default:

- **Read-only mode** (default) — write commands (`rm`, `mv`, `cp`, `mkdir`, `git push`, etc.) are blocked. Use `--allow-write` to enable.
- **Confirmation prompt** — every command requires `[y/N]` approval before execution. Use `--no-confirm` to skip (only for automated/test use).
- **Command denylist** — `rm -rf /`, `mkfs`, `dd if=`, fork bombs, `shutdown`, `reboot` are always blocked regardless of mode.
- **Environment filtering** — only safe env vars (`PATH`, `HOME`, `USER`, `SHELL`, `LANG`, `TERM`, `TZ`) are passed through. Secrets, tokens, API keys are stripped.
- **Output truncation** — stdout/stderr capped at 50KB to prevent memory blowup.
- **Working directory** — defaults to `/tmp/golem-sandbox/`, configurable via `--work-dir`.
- SQLite database is stored as a plain file. No encryption.
- No authentication or authorization on the engine yet.

## Machine

Raspberry Pi 5 (4× Cortex-A76 @ 2.4GHz, 8GB RAM), Arch Linux ARM. Built to run on constrained hardware.
