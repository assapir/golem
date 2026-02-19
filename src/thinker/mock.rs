use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Context, ModelInfo, StepResult, Thinker};

/// A scripted thinker for tests. Returns pre-defined steps in order.
pub struct MockThinker {
    steps: Vec<StepResult>,
    index: AtomicUsize,
}

impl MockThinker {
    pub fn new(steps: Vec<StepResult>) -> Self {
        Self {
            steps,
            index: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Thinker for MockThinker {
    async fn models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![])
    }

    fn model(&self) -> &str {
        "mock"
    }

    fn set_model(&mut self, _model: String) {
        // no-op for mock
    }

    async fn next_step(&self, _context: &Context) -> Result<StepResult> {
        let i = self.index.fetch_add(1, Ordering::SeqCst);
        let result = self.steps.get(i).ok_or_else(|| {
            anyhow::anyhow!("MockThinker: no more steps (called {} times)", i + 1)
        })?;
        // Clone the step, copy the usage
        Ok(StepResult {
            step: result.step.clone(),
            usage: result.usage,
        })
    }
}
