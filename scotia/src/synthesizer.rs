use crate::event::{ActionStatus, ScotiaEvent, ScotiaRun};
use std::collections::HashMap;

/// A synthesized view of a Scotia run.
#[derive(Debug, Clone, Default)]
pub struct Synthesis {
    pub summary: String,
    pub decision_rationales: Vec<String>,
    pub trade_offs: Vec<String>,
    pub action_graph_dot: String,
}

/// Generate a post-hoc synthesis from a normalized run.
pub fn synthesize(run: &ScotiaRun) -> Synthesis {
    Synthesis {
        summary: generate_summary(run),
        decision_rationales: generate_rationales(run),
        trade_offs: generate_trade_offs(run),
        action_graph_dot: generate_action_graph(run),
    }
}

fn generate_summary(run: &ScotiaRun) -> String {
    let mut actions = 0usize;
    let mut errors = 0usize;
    let mut routes: Vec<String> = Vec::new();
    let mut response_words = 0usize;

    for event in &run.events {
        match event {
            ScotiaEvent::ActionInvoked { tool, .. } => {
                actions += 1;
                if tool == "edit" || tool == "write" {
                    routes.push(format!("file mutation via {}", tool));
                }
            }
            ScotiaEvent::ErrorOrRetry { .. } => errors += 1,
            ScotiaEvent::ModelRouted { model, stage, .. } => {
                routes.push(format!("{} routed to {}", stage, model));
            }
            ScotiaEvent::ResponseChunk { content, .. } => {
                response_words += content.split_whitespace().count();
            }
            _ => {}
        }
    }

    let task = run.task.as_deref().unwrap_or("unspecified task");

    format!(
        "# Scotia Run Summary\n\n- **Agent:** {}\n- **Task:** {}\n- **Run ID:** {}\n- **Actions observed:** {}\n- **Errors/retries:** {}\n- **Response words:** {}\n- **Routing decisions:** {}\n",
        run.agent.as_str(),
        task,
        run.run_id,
        actions,
        errors,
        response_words,
        if routes.is_empty() {
            "none".to_string()
        } else {
            routes.join(", ")
        }
    )
}

fn generate_rationales(run: &ScotiaRun) -> Vec<String> {
    let mut rationales = Vec::new();

    // Heuristic: edits after reads suggest the agent inspected context before changing it.
    let mut last_read: Option<String> = None;
    for event in &run.events {
        match event {
            ScotiaEvent::ActionInvoked { tool, target, .. }
                if tool == "read" || tool == "view" || tool == "file_read" =>
            {
                last_read = target.clone();
            }
            ScotiaEvent::ActionInvoked { tool, target, .. }
                if tool == "edit" || tool == "write" || tool == "code_edit" =>
            {
                if let Some(read_target) = &last_read
                    && target.as_deref() == Some(read_target)
                {
                    rationales.push(format!(
                            "The agent likely edited {} because it had just read the same file and identified a needed change.",
                            read_target
                        ));
                }
            }
            _ => {}
        }
    }

    // Heuristic: retries indicate a prior failure that the agent attempted to recover from.
    for event in &run.events {
        if let ScotiaEvent::ErrorOrRetry {
            message,
            retry_count,
            ..
        } = event
        {
            let count = retry_count
                .map(|n| format!(" (retry {})", n))
                .unwrap_or_default();
            rationales.push(format!(
                "A retry/error occurred{}; inferred rationale: the previous attempt did not satisfy constraints and the agent is adjusting. Message: {}",
                count, message
            ));
        }
    }

    // Heuristic: model routing suggests latency/cost/quality trade-offs.
    for event in &run.events {
        if let ScotiaEvent::ModelRouted { stage, model, .. } = event {
            rationales.push(format!(
                "Stage '{}' was routed to model '{}'. Inferred rationale: this model was selected for the latency/capability profile required by that stage.",
                stage, model
            ));
        }
    }

    rationales
}

fn generate_trade_offs(run: &ScotiaRun) -> Vec<String> {
    let mut trade_offs = Vec::new();

    let mut has_remote = false;
    let mut has_local = false;
    for event in &run.events {
        if let ScotiaEvent::ModelRouted { model, .. } = event {
            let lower = model.to_lowercase();
            if lower.contains("groq") || lower.contains("openai") || lower.contains("anthropic") {
                has_remote = true;
            }
            if lower.contains("ollama") || lower.contains("local") {
                has_local = true;
            }
        }
    }

    if has_remote && has_local {
        trade_offs.push(
            "Mixed local and remote model usage detected. Trade-off: local execution preserves privacy and reduces API cost, while remote execution may improve capability and latency."
                .to_string(),
        );
    } else if has_remote {
        trade_offs.push(
            "Only remote models were used. Trade-off: this maximizes capability and speed but increases external dependency and cost."
                .to_string(),
        );
    } else if has_local {
        trade_offs.push(
            "Only local models were used. Trade-off: this preserves privacy and avoids API cost but may sacrifice capability or latency."
                .to_string(),
        );
    }

    // Look for repeated similar actions -> iterative refinement.
    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    for event in &run.events {
        if let ScotiaEvent::ActionInvoked { tool, .. } = event {
            *tool_counts.entry(tool.clone()).or_insert(0) += 1;
        }
    }
    for (tool, count) in tool_counts {
        if count > 2 {
            trade_offs.push(format!(
                "Tool '{}' was invoked {} times. Trade-off: repeated use suggests iterative exploration rather than a single-shot plan, increasing traceability at the cost of verbosity.",
                tool, count
            ));
        }
    }

    trade_offs
}

fn generate_action_graph(run: &ScotiaRun) -> String {
    let mut dot =
        String::from("digraph scotia_run {\n  rankdir=LR;\n  node [shape=box, style=rounded];\n");

    let mut node_ids: HashMap<String, String> = HashMap::new();
    let mut next_id = 0usize;

    for event in &run.events {
        match event {
            ScotiaEvent::ActionInvoked { tool, target, .. } => {
                let label = target
                    .as_ref()
                    .map(|t| format!("{}: {}", tool, t))
                    .unwrap_or_else(|| tool.clone());
                let id = format!("n{}", next_id);
                next_id += 1;
                node_ids.insert(label.clone(), id.clone());
                dot.push_str(&format!(
                    "  \"{}\" [label=\"{}\"];\n",
                    id,
                    escape_dot(&label)
                ));
            }
            ScotiaEvent::ActionResult { status, .. } => {
                let color = match status {
                    Some(ActionStatus::Success) => "green",
                    Some(ActionStatus::Failure) => "red",
                    Some(ActionStatus::Cancelled) => "orange",
                    None => "gray",
                };
                let id = format!("n{}", next_id);
                next_id += 1;
                let label = format!("result:{:?}", status);
                dot.push_str(&format!(
                    "  \"{}\" [label=\"{}\", color=\"{}\"];\n",
                    id,
                    escape_dot(&label),
                    color
                ));
            }
            _ => {}
        }
    }

    dot.push_str("}\n");
    dot
}

fn escape_dot(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}
