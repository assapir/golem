# Golem

A clay body, animated by words. Rust AI agent harness with ReAct loop, pluggable tools, and SQLite memory.

## Build and run

```bash
cargo build                           # debug build
cargo build --release                 # release build
cargo run                             # interactive REPL (default: anthropic provider)
cargo run -- --help                   # show CLI options
cargo run -- login                    # OAuth login to Anthropic
cargo run -- logout                   # remove stored credentials
cargo run -- -p human                 # use human provider (stdin)
cargo run -- --model claude-sonnet-4-20250514  # specific model
cargo run -- -r "some task"           # single task, non-interactive
cargo run -- -d custom.db             # custom SQLite path
cargo run -- -d :memory:              # ephemeral memory (no persistence)
cargo run -- -m 10 -t 60             # max 10 iterations, 60s tool timeout
cargo run -- --allow-write            # enable write operations in shell
cargo run -- --no-confirm             # skip confirmation prompts
cargo run -- -w /path/to/dir          # set shell working directory
```

### CLI reference

```
Usage: golem [OPTIONS] [COMMAND]

Commands:
  login   Log in to an LLM provider via OAuth
  logout  Log out from an LLM provider

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
```

## Testing

- Run `cargo test` before committing any change.
- Tests live in `tests/` as integration tests AND `src/` as unit tests (prompts, auth storage, anthropic parsing).
- `MockThinker` is used in tests to script agent behavior — add steps as `Vec<Step>`.
- When adding a new tool, add tests in `tests/tools_test.rs`.
- When changing the ReAct loop, add tests in `tests/react_test.rs`.
- When changing memory, add tests in `tests/memory_test.rs`.
- When changing auth, add tests in `tests/auth_test.rs`.
- When changing prompts, add tests in `src/prompts/react.rs`.
- Always test both success and error paths. Errors are observations, not crashes.

```bash
cargo test                            # run all tests
cargo test react                      # run react loop tests only
cargo test tools                      # run tool tests only
cargo test memory                     # run memory tests only
cargo test auth                       # run auth tests only
cargo test prompt                     # run prompt tests only
```

## Code style

- Run `cargo clippy` and fix all warnings before committing.
- Run `cargo fmt` before committing — CI enforces `cargo fmt --check`.
- All public traits must be `Send + Sync` — this is required for async + parallel tool execution.
- Use `Arc` for shared ownership across async boundaries, `RwLock` for mutable shared state.
- Errors from tool execution are **never** panics or `Result::Err` propagated to the loop. They become `Outcome::Error(String)` and get fed back to the thinker as observations.
- Prefer `async` functions. Tools, memory, and thinkers are all async traits.
- Never hardcode version strings — use `env!("CARGO_PKG_VERSION")`.
- Never write files that can be generated — use `cargo init`, `cargo add`, etc.
- `main.rs` imports from the library crate (`use golem::...`), not `mod` declarations. This avoids duplicate compilation and dead_code warnings.

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
| `Thinker` | Produces next `StepResult` given context | `HumanThinker`, `AnthropicThinker`, `MockThinker` |
| `Tool` | Executes an action, returns string | `ShellTool` |
| `Memory` | Stores and retrieves `MemoryEntry` | `SqliteMemory` |

### Module layout

```
src/
├── main.rs              # CLI, wiring, REPL, login/logout subcommands
├── lib.rs               # re-exports all modules for library use
├── auth/
│   ├── mod.rs           # re-exports AuthStorage, Credential
│   ├── oauth.rs         # Anthropic OAuth PKCE flow (authorize, token exchange, refresh)
│   └── storage.rs       # credential storage (~/.golem/auth.json), API key fallback
├── engine/
│   ├── mod.rs           # Engine trait
│   └── react.rs         # ReactEngine (ReAct loop)
├── prompts/
│   ├── mod.rs           # re-exports build_react_system_prompt
│   └── react.rs         # shared ReAct system prompt builder, used by all LLM providers
├── thinker/
│   ├── mod.rs           # Thinker trait, Step, ToolCall, Context, ToolDescription
│   ├── anthropic.rs     # Anthropic Messages API (OAuth + API key auth)
│   ├── human.rs         # interactive stdin thinker
│   └── mock.rs          # scripted thinker for tests
├── tools/
│   ├── mod.rs           # Tool trait, ToolRegistry, ToolResult, Outcome
│   └── shell.rs         # shell command execution with security controls
└── memory/
    ├── mod.rs           # Memory trait, MemoryEntry
    └── sqlite.rs        # SQLite-backed persistent memory

tests/
├── auth_test.rs         # auth storage CRUD, permissions, API key resolution
├── memory_test.rs       # SQLite memory persistence tests
├── react_test.rs        # ReAct loop integration tests
└── tools_test.rs        # shell tool security + execution tests
```

## Authentication

Golem supports two auth methods for Anthropic:

### OAuth (recommended)
```bash
cargo run -- login                    # opens browser for OAuth PKCE flow
```
- Uses Anthropic's OAuth PKCE flow (same as Claude Code / pi)
- Tokens stored in `~/.golem/auth.json` with `0600` permissions
- Auto-refreshes expired tokens using refresh token
- OAuth tokens detected by `sk-ant-oat` prefix → uses `Authorization: Bearer` header
- Required headers: `anthropic-beta`, `user-agent`, `x-app` (matches Claude Code client)

### API key
```bash
export ANTHROPIC_API_KEY=sk-ant-api03-...
cargo run                             # auto-detects API key from env
```
- Falls back to `ANTHROPIC_API_KEY` env var if no OAuth credentials
- Uses `x-api-key` header (standard Anthropic API auth)

### Credential resolution order
1. `~/.golem/auth.json` OAuth token (refreshed if expired)
2. `ANTHROPIC_API_KEY` environment variable
3. Error: "not authenticated"

## System prompt

The ReAct system prompt lives in `src/prompts/react.rs` and is shared across all LLM providers. Design principles:

- **No markdown fences** — prevents models from wrapping output in fences
- **Pretty-printed JSON examples** — readable structure the model can follow
- **Strict output rules**: JSON only, no extra keys, brief thoughts (1-2 sentences)
- **Tool discipline**: only listed tools, match expected args exactly, never invent tools
- **Direct answers**: skip tools when the task can be answered without them
- **Parallel calls**: multiple tools via the `calls` array

When adding a new provider, use `build_react_system_prompt(&context.available_tools)` — don't duplicate the prompt.

## Design decisions

- **Trait objects over generics** — runtime swappability (`set_thinker()`, heterogeneous `ToolRegistry`) requires dynamic dispatch. Vtable cost is nanoseconds vs seconds of LLM latency.
- **Keep `async_trait`** — `async_fn_in_dyn_trait` is unstable even on nightly. Tracking: rust-lang/rust#133119.
- **`Thinker::next_step` returns `StepResult`** — wraps `Step` + `Option<TokenUsage>`. Non-LLM thinkers return `None`. The engine accumulates usage.
- **`Step::Act` always has `Vec<ToolCall>`** — a single call is `vec![one]`, no separate parallel variant.
- **`ToolResult` contains `Outcome::Success` or `Outcome::Error`** — errors are information, not failures. The thinker decides what to do.
- **The ReAct loop only enforces hard limits** — max iterations and tool timeout. All intelligence is in the thinker.
- **Thinker is swappable at runtime** via `engine.set_thinker()`. The `Arc<RwLock<...>>` wrapper makes this safe between iterations.
- **Provider ≠ Model** — `--provider` selects the API (human, anthropic), `--model` selects which model on that provider.
- **ToolRegistry uses `RwLock`** — supports runtime tool registration/unregistration for self-modification.
- **Default provider is `anthropic`** — the agent is built for LLM use; human provider is for debugging.

## Adding a new tool

1. Create `src/tools/my_tool.rs`, implement the `Tool` trait.
2. Register it in `main.rs`: `tools.register(Arc::new(MyTool)).await;`
3. Add tests in `tests/tools_test.rs`.
4. The tool must be `Send + Sync` and its `execute` must be `async`.

## Adding a new thinker (provider)

1. Create `src/thinker/my_provider.rs`, implement the `Thinker` trait.
2. Use `build_react_system_prompt()` from `src/prompts/react.rs` for the system prompt.
3. Add a variant to the `Provider` enum in `main.rs`.
4. Add the match arm to construct the thinker.
5. Test with `MockThinker` patterns in `tests/react_test.rs`.

## CI/CD

### CI (`ci.yml`)
Runs on PRs and push to main (skips version bump commits):
- `cargo fmt --check`
- `cargo clippy --all-targets` (warnings are errors via `RUSTFLAGS=-D warnings`)
- `cargo test`

### Release (`release.yml`)
Runs on push to main (skips version bump commits) + manual `workflow_dispatch`:
1. **Version bump**: reads Cargo.toml, bumps minor (default), commits + tags
2. **Build**: 4-platform matrix (x86_64/aarch64 × linux/macOS), uses `cross` for aarch64-linux
3. **Release**: creates GitHub Release with binaries + SHA-256 checksums

```bash
# Manual override (from CLI or GitHub UI):
gh workflow run release.yml -f bump=patch   # 0.1.0 → 0.1.1
gh workflow run release.yml -f bump=minor   # 0.1.0 → 0.2.0 (default)
gh workflow run release.yml -f bump=major   # 0.1.0 → 1.0.0
```

Version is auto-managed in Cargo.toml — don't edit it manually.

### AUR package (`golem-bin`)

The `aur/` directory contains the PKGBUILD for the Arch Linux AUR binary package.

- `aur/PKGBUILD` — downloads pre-built binary from GitHub releases (x86_64 + aarch64)
- `aur/.SRCINFO` — generated metadata (`makepkg --printsrcinfo > .SRCINFO`)
- `aur/update-pkgbuild.sh` — helper script to update version + checksums locally

The release workflow's `aur` job auto-publishes to AUR after creating a GitHub Release.
Requires `AUR_SSH_KEY` secret (see issue #12).

```bash
# Install from AUR
yay -S golem-bin

# Test PKGBUILD locally
cd aur && makepkg -sf

# Update PKGBUILD manually
./aur/update-pkgbuild.sh 0.2.0
```

## Startup banner

On launch (both REPL and single-task mode), golem prints an informative banner:

```
   ╔═══════════════════════════════════════╗
   ║              G O L E M                ║
   ║     a clay body, animated by words    ║
   ╚═══════════════════════════════════════╝

   version   0.8.0
   provider  anthropic (claude-sonnet-4-20250514)
   auth      OAuth ✓
   shell     read-only
   workdir   /tmp/golem-sandbox
   memory    golem.db
```

The banner shows current provider, model, auth status, shell mode, working directory, and memory backend at a glance.

## Token tracking

Token usage is tracked cumulatively across the entire session:

- Each `Thinker::next_step()` returns a `StepResult` containing an optional `TokenUsage` (input + output tokens).
- `ReactEngine` accumulates usage across all iterations and tasks via `session_usage()`.
- On exit (or after single-task mode), a session summary is printed:
  ```
  session:    323 input +     45 output =    368 tokens
  goodbye.
  ```
- Non-LLM thinkers (human, mock) return `None` for usage — no summary line printed when total is zero.
- Per-call token logging is removed; only the session total is shown.

## REPL signal handling

The REPL uses async stdin (`tokio::io::BufReader` + `AsyncBufReadExt::lines()`) so that signal handling works at every point:

| Input | At the prompt | During task execution |
|-------|--------------|----------------------|
| **Ctrl+C** | Exits REPL with `goodbye.` | Cancels the running task, returns to prompt |
| **Ctrl+D** | Exits REPL with `goodbye.` | N/A |
| **`quit`/`exit`** | Exits REPL with `goodbye.` | N/A |

Both the prompt read and the engine run are wrapped in `tokio::select!` against `tokio::signal::ctrl_c()`. Synchronous stdin would block the tokio runtime and prevent signal interception.

## Security model

The shell tool is locked down by default:

- **Read-only mode** (default) — write commands (`rm`, `mv`, `cp`, `mkdir`, `git push`, etc.) are blocked. Use `--allow-write` to enable.
- **Confirmation prompt** — every command requires `[y/N]` approval before execution. Use `--no-confirm` to skip (only for automated/test use).
- **Command denylist** — `rm -rf /`, `mkfs`, `dd if=`, fork bombs, `shutdown`, `reboot` are always blocked regardless of mode.
- **Environment filtering** — only safe env vars (`PATH`, `HOME`, `USER`, `SHELL`, `LANG`, `TERM`, `TZ`) are passed through. Secrets, tokens, API keys are stripped.
- **Output truncation** — stdout/stderr capped at 50KB to prevent memory blowup.
- **Working directory** — defaults to `/tmp/golem-sandbox/`, configurable via `--work-dir`.
- Auth tokens stored in `~/.golem/auth.json` with `0600` permissions (owner read/write only).
- SQLite database is stored as a plain file. No encryption.
- No authentication or authorization on the engine yet.

## Workflow

- **Always use worktrees** — never edit main directly. Create a feature branch in `.worktrees/`, push, open a PR, merge via squash.
- **Always update docs when finishing a feature** — before opening a PR, update AGENTS.md, README.md, and any other relevant docs to reflect the changes. Outdated docs are bugs.
- **Version is auto-managed** — don't edit `Cargo.toml` version or `aur/PKGBUILD` version manually. The CD pipeline handles it.

## Machine

Raspberry Pi 5 (4× Cortex-A76 @ 2.4GHz, 8GB RAM), Arch Linux ARM. Built to run on constrained hardware.
