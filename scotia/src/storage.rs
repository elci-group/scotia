use crate::event::ScotiaRun;
use crate::normalizer::normalize;
use crate::synthesizer::synthesize;
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

/// Maximum size of a run JSON file that `load_run` will read into memory.
/// 64 MiB is vastly larger than any legitimate run and bounds the cost of
/// deserialising a user-supplied path (`replay` / `summary` / `validate` / ...).
const MAX_RUN_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Configuration for the Scotia log store.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub root: PathBuf,
    pub commit_to_git: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("scotia-log"),
            commit_to_git: false,
        }
    }
}

/// Persist a run to the Scotia log store.
pub async fn store_run(config: &StorageConfig, run: ScotiaRun) -> Result<StoredRun> {
    let normalized = normalize(run);
    let synthesis = synthesize(&normalized);

    let date_dir = format!("{}", Utc::now().format("%Y-%m-%d"));
    let run_dir = config.root.join(&date_dir);
    tokio::fs::create_dir_all(&run_dir)
        .await
        .with_context(|| format!("failed to create run directory {:?}", run_dir))?;

    let base = format!(
        "run_{}",
        normalized
            .run_id
            .to_string()
            .split('-')
            .next()
            .unwrap_or("unknown")
    );

    let json_path = run_dir.join(format!("{}.json", base));
    let summary_path = run_dir.join(format!("{}.summary.md", base));
    let dot_path = run_dir.join(format!("{}.dot", base));

    let json =
        serde_json::to_string_pretty(&normalized).context("failed to serialize run to JSON")?;
    tokio::fs::write(&json_path, json)
        .await
        .with_context(|| format!("failed to write {}", json_path.display()))?;

    let mut summary = synthesis.summary;
    if !synthesis.decision_rationales.is_empty() {
        summary.push_str("\n## Decision Rationales\n\n");
        for r in &synthesis.decision_rationales {
            summary.push_str(&format!("- {}\n", r));
        }
    }
    if !synthesis.trade_offs.is_empty() {
        summary.push_str("\n## Trade-offs\n\n");
        for t in &synthesis.trade_offs {
            summary.push_str(&format!("- {}\n", t));
        }
    }
    tokio::fs::write(&summary_path, summary)
        .await
        .with_context(|| format!("failed to write {}", summary_path.display()))?;

    tokio::fs::write(&dot_path, &synthesis.action_graph_dot)
        .await
        .with_context(|| format!("failed to write {}", dot_path.display()))?;

    if config.commit_to_git {
        crate::git::commit_artifact(
            config.root.parent().unwrap_or(&config.root),
            &run_dir,
            &base,
        )
        .await?;
    }

    Ok(StoredRun {
        run_id: normalized.run_id,
        json_path,
        summary_path,
        dot_path,
    })
}

/// Load a run back from disk.
pub async fn load_run(path: &Path) -> Result<ScotiaRun> {
    let meta = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("failed to stat {}", path.display()))?;
    if meta.len() > MAX_RUN_FILE_BYTES {
        anyhow::bail!(
            "run file {} is {} bytes, exceeding the {} byte limit",
            path.display(),
            meta.len(),
            MAX_RUN_FILE_BYTES
        );
    }

    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read {}", path.display()))?;
    let run: ScotiaRun = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse run at {}", path.display()))?;
    Ok(run)
}

/// Find all stored run JSON files under the configured root.
pub async fn list_runs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut runs = Vec::new();
    let mut entries = tokio::fs::read_dir(root).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            let mut sub = tokio::fs::read_dir(entry.path()).await?;
            while let Some(sub_entry) = sub.next_entry().await? {
                let path = sub_entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    runs.push(path);
                }
            }
        }
    }
    runs.sort();
    Ok(runs)
}

#[derive(Debug, Clone)]
pub struct StoredRun {
    pub run_id: uuid::Uuid,
    pub json_path: PathBuf,
    pub summary_path: PathBuf,
    pub dot_path: PathBuf,
}
