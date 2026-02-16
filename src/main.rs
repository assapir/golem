mod engine;
mod memory;
mod thinker;
mod tools;

use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, ValueEnum};

use engine::react::{ReactConfig, ReactEngine};
use engine::Engine;
use memory::sqlite::SqliteMemory;
use thinker::human::HumanThinker;
use thinker::Thinker;
use tools::shell::ShellTool;
use tools::ToolRegistry;

#[derive(Debug, Clone, ValueEnum)]
enum Provider {
    Human,
    // Ollama, // later
}

#[derive(Parser)]
#[command(name = "golem", version, about = "A clay body, animated by words.")]
struct Cli {
    /// LLM provider
    #[arg(short, long, value_enum, default_value_t = Provider::Human)]
    provider: Provider,

    /// Model name (provider-specific, ignored for human)
    #[arg(long)]
    model: Option<String>,

    /// SQLite database path for memory persistence (use :memory: for ephemeral)
    #[arg(short, long, default_value = ":memory:")]
    db: String,

    /// Maximum ReAct loop iterations before giving up
    #[arg(short, long, default_value_t = 20)]
    max_iterations: usize,

    /// Tool execution timeout in seconds
    #[arg(short, long, default_value_t = 30)]
    timeout: u64,

    /// Run a single task and exit (non-interactive)
    #[arg(short, long)]
    run: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("golem v{} — a clay body, animated by words\n", env!("CARGO_PKG_VERSION"));

    // Wire up the thinker based on provider + model
    let thinker: Box<dyn Thinker> = match cli.provider {
        Provider::Human => {
            if cli.model.is_some() {
                eprintln!("warning: --model is ignored for human provider");
            }
            Box::new(HumanThinker)
        }
    };

    let tools = Arc::new(ToolRegistry::new());
    tools.register(Arc::new(ShellTool)).await;

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
        return Ok(());
    }

    // REPL
    loop {
        print!("\ngolem> ");
        io::stdout().flush()?;

        let mut task = String::new();
        io::stdin().read_line(&mut task)?;
        let task = task.trim();

        if task.is_empty() {
            continue;
        }
        if task == "quit" || task == "exit" {
            break;
        }

        match engine.run(task).await {
            Ok(answer) => println!("\n=> {}", answer),
            Err(e) => eprintln!("\nerror: {}", e),
        }
    }

    Ok(())
}
