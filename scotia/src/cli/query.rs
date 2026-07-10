//! Read-only run-inspection subcommands: replay, summary, list, validate,
//! diff, regression. Extracted from `cli.rs` so the top-level dispatcher is not
//! a god object; these are pure consumers of stored runs.

use crate::algebra::{diff_runs, regression_suite, render_regression_suite, validate};
use crate::storage::{StorageConfig, list_runs, load_run};
use anyhow::Result;
use std::path::Path;

pub async fn replay(path: &Path) -> Result<()> {
    let run = load_run(path).await?;
    for event in run.events {
        println!("{}", serde_json::to_string(&event)?);
    }
    Ok(())
}

pub async fn summary(path: &Path) -> Result<()> {
    let run = load_run(path).await?;
    let synthesis = crate::synthesizer::synthesize(&run);
    println!("{}", synthesis.summary);
    if !synthesis.decision_rationales.is_empty() {
        println!("\n## Decision Rationales");
        for r in &synthesis.decision_rationales {
            println!("- {}", r);
        }
    }
    if !synthesis.trade_offs.is_empty() {
        println!("\n## Trade-offs");
        for t in &synthesis.trade_offs {
            println!("- {}", t);
        }
    }
    Ok(())
}

pub async fn list(storage: &StorageConfig) -> Result<()> {
    let runs = list_runs(&storage.root).await?;
    if runs.is_empty() {
        println!("No Scotia runs found under {}", storage.root.display());
    } else {
        for run in runs {
            println!("{}", run.display());
        }
    }
    Ok(())
}

pub async fn validate_run(path: &Path) -> Result<()> {
    let run = load_run(path).await?;
    let issues = validate(&run);
    if issues.is_empty() {
        println!("Run {} is structurally valid.", run.run_id);
    } else {
        println!(
            "Run {} has {} validation issue(s):",
            run.run_id,
            issues.len()
        );
        for issue in issues {
            println!("  - {:?}", issue);
        }
    }
    Ok(())
}

pub async fn diff(left: &Path, right: &Path) -> Result<()> {
    let left_run = load_run(left).await?;
    let right_run = load_run(right).await?;
    let diff = diff_runs(&left_run, &right_run);
    println!("Actions added:    {:?}", diff.actions_added);
    println!("Actions removed:  {:?}", diff.actions_removed);
    println!("Models added:     {:?}", diff.models_added);
    println!("Models removed:   {:?}", diff.models_removed);
    println!("Errors added:     {}", diff.errors_added);
    println!("Errors removed:   {}", diff.errors_removed);
    Ok(())
}

pub async fn regression(path: &Path) -> Result<()> {
    let run = load_run(path).await?;
    let suite = regression_suite(&run);
    println!("{}", render_regression_suite(&suite));
    Ok(())
}
