use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::io::{self, Write};

use super::{Context, Step, Thinker, ToolCall};

/// You are the brain. Type thoughts and actions at the terminal.
pub struct HumanThinker;

impl HumanThinker {
    fn read_line(prompt: &str) -> Result<String> {
        print!("{}", prompt);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_string())
    }

    fn print_context(context: &Context) {
        println!("\n{}", "=".repeat(60));
        println!("Task: {}", context.task);
        println!("{}", "-".repeat(60));

        if !context.history.is_empty() {
            println!("History:");
            for (i, entry) in context.history.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("  {}", entry);
            }
            println!("{}", "-".repeat(60));
        }

        println!("Available tools:");
        for tool in &context.available_tools {
            println!("  {} — {}", tool.name, tool.description);
        }
        println!("{}", "=".repeat(60));
    }
}

#[async_trait]
impl Thinker for HumanThinker {
    async fn next_step(&self, context: &Context) -> Result<Step> {
        Self::print_context(context);

        let thought = Self::read_line("\nThought: ")?;
        let action = Self::read_line("Action (tool:arg or 'finish'): ")?;

        if action == "finish" {
            let answer = Self::read_line("Answer: ")?;
            return Ok(Step::Finish { thought, answer });
        }

        // Parse "tool:arg" or "tool:key=val,key=val"
        let calls = action
            .split(';')
            .map(|call| {
                let call = call.trim();
                let (tool, args_str) = call
                    .split_once(':')
                    .unwrap_or((call, ""));

                let mut args = HashMap::new();
                if !args_str.is_empty() {
                    // If no '=' present, treat the whole thing as "command" arg
                    if args_str.contains('=') {
                        for pair in args_str.split(',') {
                            if let Some((k, v)) = pair.split_once('=') {
                                args.insert(k.trim().to_string(), v.trim().to_string());
                            }
                        }
                    } else {
                        args.insert("command".to_string(), args_str.to_string());
                    }
                }

                ToolCall {
                    tool: tool.to_string(),
                    args,
                }
            })
            .collect();

        Ok(Step::Act { thought, calls })
    }
}
