use anyhow::{Context, Result};
use git2::{Repository, Signature};
use std::path::Path;

/// Commit the Scotia artifact as a single deterministic commit in the current repo.
///
/// The commit is made to the repository that contains `repo_root`. If no
/// repository exists there, this function returns an error.
pub async fn commit_artifact(repo_root: &Path, artifact_dir: &Path, base_name: &str) -> Result<()> {
    let repo = Repository::open(repo_root)
        .with_context(|| format!("no git repository at {}", repo_root.display()))?;

    let mut index = repo.index().context("failed to open git index")?;

    // Add all files under the artifact dir.
    add_path_recursive(&repo, &mut index, artifact_dir, repo_root)?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let sig = Signature::now("Scotia", "scotia@localhost")?;
    let parent = repo.head()?.peel_to_commit()?;
    let message = format!("scotia: add decision ledger for run {}", base_name);

    repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])
        .with_context(|| "failed to commit Scotia artifact")?;

    Ok(())
}

fn add_path_recursive(
    _repo: &Repository,
    index: &mut git2::Index,
    path: &Path,
    repo_root: &Path,
) -> Result<()> {
    let rel = path.strip_prefix(repo_root).with_context(|| {
        format!(
            "path {} is not inside repo {}",
            path.display(),
            repo_root.display()
        )
    })?;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            add_path_recursive(_repo, index, &entry.path(), repo_root)?;
        }
    } else {
        index.add_path(rel)?;
    }
    Ok(())
}
