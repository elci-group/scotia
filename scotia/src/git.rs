use anyhow::{Context, Result};
use git2::{Repository, RepositoryState, Signature};
use std::path::Path;

/// Commit the Scotia artifact as a single deterministic commit in the current repo.
///
/// The commit is made to the repository that contains `repo_root`. If no
/// repository exists there, this function returns an error.
pub async fn commit_artifact(repo_root: &Path, artifact_dir: &Path, base_name: &str) -> Result<()> {
    let repo = Repository::open(repo_root)
        .with_context(|| format!("no git repository at {}", repo_root.display()))?;

    // Refuse to commit while a merge / rebase / cherry-pick / bisect is in
    // progress: writing HEAD here would silently clobber the in-progress state.
    let state = repo.state();
    if state != RepositoryState::Clean {
        anyhow::bail!(
            "refusing to commit Scotia artifact: repository is in state {:?} \
             (finish or abort the in-progress operation first)",
            state
        );
    }

    let mut index = repo.index().context("failed to open git index")?;

    // Add all files under the artifact dir.
    add_path_recursive(&repo, &mut index, artifact_dir, repo_root)?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // Prefer the repository's configured author identity; fall back to a clear
    // Scotia identity only when none is configured, so we never impersonate the
    // user but also never fail just because identity is unset.
    let (name, email) = configured_identity(&repo)
        .unwrap_or_else(|| ("Scotia".to_string(), "scotia@localhost".to_string()));
    let sig = Signature::now(&name, &email)
        .with_context(|| format!("invalid git author identity: {} <{}>", name, email))?;

    // Support an unborn branch (fresh repository with no commits): in that case
    // there is no parent and this becomes the initial commit on HEAD.
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();

    let message = format!("scotia: add decision ledger for run {}", base_name);

    repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &parents)
        .with_context(|| "failed to commit Scotia artifact")?;

    Ok(())
}

/// Read `user.name` / `user.email` from the repository's effective config.
fn configured_identity(repo: &Repository) -> Option<(String, String)> {
    let cfg = repo.config().ok()?;
    let name = cfg.get_string("user.name").ok()?;
    let email = cfg.get_string("user.email").ok()?;
    if name.is_empty() || email.is_empty() {
        return None;
    }
    Some((name, email))
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
