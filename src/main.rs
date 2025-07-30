use std::{collections::HashMap, path::PathBuf};

use anyhow::bail;
use clap::Parser;

/// Estimate hours of a project
#[derive(Debug, Parser, Clone)]
#[command(version, long_about = None)]
struct Args {
    /// Maximum time difference between two subsequent commits in minutes which are counted to be
    /// in the same coding "session"
    #[arg(short = 'd', long, default_value_t = 2 * 60)]
    max_commit_diff: u32,

    /// How many minutes should be added for the first commit of the coding session
    #[arg(short, long, default_value_t = 2 * 60)]
    first_commit_add: u32,

    // /// Include commits since
    // #[arg(short, long)]
    // since:,
    // /// Include commits until
    // #[arg(short, long)]
    // until:,
    /// Include merge commits (commits with more than one parent)
    #[arg(short, long, default_value_t = true)]
    merge_commits: bool,

    /// Git repository
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    /// Aliases of emails for grouping the same activity as one person
    // #[arg(short, long)]
    // email_aliases: HashMap<String, String>,

    /// Git branch
    #[arg(short, long)]
    branch: Option<String>,
}

fn get_commits(
    args: &Args,
    repo: &gix::Repository,
) -> anyhow::Result<HashMap<gix::ObjectId, (String, String, gix::date::Time)>> {
    let refs = repo.references()?;
    let prefix = if let Some(branch) = &args.branch {
        format!("refs/heads/{branch}")
    } else {
        "refs/heads/".to_string()
    };
    let heads = refs.prefixed(prefix.as_str())?;

    let mut commits = HashMap::new();
    for head in heads.filter_map(|h| h.ok()) {
        let mut stack = vec![head.id()];
        while let Some(id) = stack.pop() {
            let Ok(commit) = repo.find_commit(id) else {
                continue;
            };

            if commits.contains_key(&commit.id) {
                // This commit and its parents have already been visited. Any further work is
                // redundant.
                continue;
            }

            let stack_len = stack.len();
            // extend the stack directly to avoid allocating a temporary vec for the parents.
            stack.extend(commit.parent_ids());
            let num_parents = stack.len() - stack_len;

            if let Ok(author) = commit.author()
                && let Ok(time) = author.time()
            {
                let is_merge = num_parents > 1;
                if !is_merge || args.merge_commits {
                    // TODO: filter by since/until
                    commits.insert(
                        commit.id,
                        (author.name.to_string(), author.email.to_string(), time),
                    );
                }
            }
        }
    }

    Ok(commits)
}

fn main() -> anyhow::Result<()> {
    if std::fs::exists(".git/shallow")? {
        bail!(
            "Cannot analyze shallow copies. Please run `git fetch --unshallow` before continuing."
        );
    }

    let args = Args::parse();
    let repo = gix::open(&args.path)?;
    let commits = get_commits(&args, &repo)?;

    Ok(())
}
