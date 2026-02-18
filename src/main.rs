use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use tokio::io::{AsyncBufReadExt, BufReader};

use golem::auth::oauth;
use golem::auth::storage::{AuthStorage, Credential};
use golem::banner::{BannerInfo, print_banner, print_session_summary};
use golem::consts::DEFAULT_MODEL;
use golem::engine::Engine;
use golem::engine::react::{ReactConfig, ReactEngine};
use golem::memory::sqlite::SqliteMemory;
use golem::thinker::Thinker;
use golem::thinker::anthropic::AnthropicThinker;
use golem::thinker::human::HumanThinker;
use golem::tools::ToolRegistry;
use golem::tools::shell::{ShellConfig, ShellMode, ShellTool};

#[derive(Debug, Clone, ValueEnum)]
enum Provider {
    Human,
    Anthropic,
}

#[derive(Parser)]
#[command(name = "golem", version, about = "A clay body, animated by words.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// LLM provider
    #[arg(short, long, value_enum, default_value_t = Provider::Anthropic)]
    provider: Provider,

    /// Model name (provider-specific, ignored for human)
    #[arg(long)]
    model: Option<String>,

    /// SQLite database path for memory persistence (use :memory: for ephemeral)
    #[arg(short, long, default_value = "golem.db")]
    db: String,

    /// Maximum ReAct loop iterations before giving up
    #[arg(short, long, default_value_t = 20)]
    max_iterations: usize,

    /// Tool execution timeout in seconds
    #[arg(short, long, default_value_t = 30)]
    timeout: u64,

    /// Allow write operations in shell tool (default: read-only)
    #[arg(long, default_value_t = false)]
    allow_write: bool,

    /// Working directory for shell commands
    #[arg(short, long)]
    work_dir: Option<PathBuf>,

    /// Skip confirmation prompts before executing commands
    #[arg(long, default_value_t = false)]
    no_confirm: bool,

    /// Run a single task and exit (non-interactive)
    #[arg(short, long)]
    run: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Log in to an LLM provider via OAuth
    Login {
        /// Provider to log in to
        #[arg(value_enum, default_value_t = LoginProvider::Anthropic)]
        provider: LoginProvider,
    },
    /// Log out from an LLM provider
    Logout {
        /// Provider to log out from
        #[arg(value_enum, default_value_t = LoginProvider::Anthropic)]
        provider: LoginProvider,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum LoginProvider {
    Anthropic,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle subcommands
    if let Some(command) = &cli.command {
        match command {
            Command::Login { provider } => {
                return handle_login(provider).await;
            }
            Command::Logout { provider } => {
                return handle_logout(provider);
            }
        }
    }

    // Wire up the thinker based on provider + model
    let (thinker, provider_name, model_name, auth_status): (
        Box<dyn Thinker>,
        &str,
        String,
        String,
    ) = match cli.provider {
        Provider::Human => {
            if cli.model.is_some() {
                eprintln!("warning: --model is ignored for human provider");
            }
            (
                Box::new(HumanThinker),
                "human",
                "—".to_string(),
                "N/A".to_string(),
            )
        }
        Provider::Anthropic => {
            let auth = AuthStorage::new()?;
            let auth_status = match auth.get("anthropic")? {
                Some(Credential::OAuth(_)) => "OAuth ✓".to_string(),
                Some(Credential::ApiKey { .. }) => "API key ✓".to_string(),
                None => {
                    if std::env::var("ANTHROPIC_API_KEY")
                        .map(|k| !k.is_empty())
                        .unwrap_or(false)
                    {
                        "API key (env) ✓".to_string()
                    } else {
                        "not authenticated".to_string()
                    }
                }
            };
            let model = cli
                .model
                .clone()
                .unwrap_or_else(|| DEFAULT_MODEL.to_string());
            let thinker = Box::new(AnthropicThinker::new(cli.model, auth)?);
            (thinker, "anthropic", model, auth_status)
        }
    };

    let shell_mode = if cli.allow_write {
        ShellMode::ReadWrite
    } else {
        ShellMode::ReadOnly
    };
    let working_dir = cli
        .work_dir
        .unwrap_or_else(|| std::env::temp_dir().join("golem-sandbox"));

    let shell_config = ShellConfig {
        mode: shell_mode,
        working_dir: working_dir.clone(),
        require_confirmation: !cli.no_confirm,
        ..ShellConfig::default()
    };

    let memory_label = if cli.db == ":memory:" {
        "ephemeral"
    } else {
        &cli.db
    };

    let shell_label = if shell_mode == ShellMode::ReadWrite {
        "read-write"
    } else {
        "read-only"
    };

    print_banner(&BannerInfo {
        provider: provider_name,
        model: &model_name,
        auth_status: &auth_status,
        shell_mode: shell_label,
        working_dir: &working_dir,
        memory: memory_label,
    });

    let tools = Arc::new(ToolRegistry::new());
    tools.register(Arc::new(ShellTool::new(shell_config))).await;

    let memory = Box::new(SqliteMemory::new(&cli.db)?);

    let config = ReactConfig {
        max_iterations: cli.max_iterations,
        tool_timeout: Duration::from_secs(cli.timeout),
    };

    let mut engine = ReactEngine::new(thinker, tools, memory, config);

    // Single task mode
    if let Some(task) = cli.run {
        match engine.run(&task).await {
            Ok(answer) => println!("\n=> {}", answer),
            Err(e) => eprintln!("\nerror: {}", e),
        }
        print_session_summary(engine.session_usage());
        return Ok(());
    }

    // REPL — async stdin so Ctrl+C is caught at the prompt too
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        print!("\ngolem> ");
        io::stdout().flush()?;

        // Read next line, interruptible by Ctrl+C
        let line = tokio::select! {
            result = lines.next_line() => {
                match result {
                    Ok(Some(line)) => line,
                    Ok(None) => {
                        // Ctrl+D (EOF)
                        println!();
                        break;
                    }
                    Err(e) => {
                        eprintln!("input error: {}", e);
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!();
                break;
            }
        };

        let task = line.trim();

        if task.is_empty() {
            continue;
        }
        if task == "quit" || task == "exit" {
            break;
        }

        // Ctrl+C during task execution cancels the task, not the REPL
        tokio::select! {
            result = engine.run(task) => {
                match result {
                    Ok(answer) => println!("\n=> {}", answer),
                    Err(e) => eprintln!("\nerror: {}", e),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n\ninterrupted");
            }
        }
    }

    print_session_summary(engine.session_usage());
    Ok(())
}

async fn handle_login(provider: &LoginProvider) -> anyhow::Result<()> {
    match provider {
        LoginProvider::Anthropic => {
            println!("Logging in to Anthropic (Claude Pro/Max)...\n");

            let (url, verifier) = oauth::build_authorize_url();

            // Try to open browser, silently ignore failures (e.g. headless/SSH)
            let _ = open::that(&url);

            println!("Open this URL to authenticate:\n");
            println!("  {}\n", url);

            print!("Paste the authorization code: ");
            io::stdout().flush()?;
            let mut code = String::new();
            io::stdin().read_line(&mut code)?;
            let code = code.trim();

            if code.is_empty() {
                anyhow::bail!("no authorization code provided");
            }

            println!("\nExchanging code for tokens...");
            let credentials = oauth::exchange_code(code, &verifier).await?;

            let storage = AuthStorage::new()?;
            storage.set("anthropic", Credential::OAuth(credentials))?;

            println!("✓ Logged in to Anthropic successfully!");
            println!("  Credentials saved to ~/.golem/auth.json");
        }
    }
    Ok(())
}

fn handle_logout(provider: &LoginProvider) -> anyhow::Result<()> {
    match provider {
        LoginProvider::Anthropic => {
            let storage = AuthStorage::new()?;
            storage.remove("anthropic")?;
            println!("✓ Logged out from Anthropic.");
        }
    }
    Ok(())
}
