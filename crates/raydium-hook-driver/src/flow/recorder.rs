//! Records what a flow did, printing each step and keeping it as evidence.

use crate::env::Evidence;

pub(super) struct Recorder {
    pub(super) flow: &'static str,
    pub(super) evidence: Vec<Evidence>,
}

impl Recorder {
    pub(super) fn new(flow: &'static str) -> Self {
        Self {
            flow,
            evidence: Vec::new(),
        }
    }

    pub(super) fn push(
        &mut self,
        step: &str,
        signature: Option<String>,
        detail: impl Into<String>,
    ) {
        let detail = detail.into();
        println!(
            "  [{}] {step}{}{}",
            self.flow,
            signature
                .as_ref()
                .map(|s| format!("  sig {s}"))
                .unwrap_or_default(),
            if detail.is_empty() {
                String::new()
            } else {
                format!("  ({detail})")
            }
        );
        self.evidence.push(Evidence {
            flow: self.flow.to_string(),
            step: step.to_string(),
            signature,
            detail,
        });
    }
}
