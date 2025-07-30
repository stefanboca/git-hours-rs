use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use anyhow::bail;
use clap::Parser;
use gix::bstr::BString;

/// Estimate hours of a project
#[derive(Debug, Parser, Clone)]
#[command(version, long_about = None)]
struct Args {
    /// Maximum time difference between two subsequent commits in minutes which are counted to be
    /// in the same coding session
    #[arg(short = 'd', long, default_value_t = 2 * 60)]
    max_commit_diff: u32,

    /// How many minutes should be added for the first commit of each coding session
    #[arg(short, long, default_value_t = 2 * 60)]
    first_commit_add: u32,

    // /// Include commits since
    // #[arg(short, long)]
    // since:,
    // /// Include commits until
    // #[arg(short, long)]
    // until:,

    // TODO: consider making flag instead of value
    /// Include merge commits (commits with more than one parent)
    #[arg(short, long, default_value_t = true)]
    merge_commits: bool,

    /// Git repository
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    // Aliases of emails for grouping the same activity as one person
    // #[arg(short, long)]
    // email_aliases: HashMap<String, String>,
    /// Git branch
    #[arg(short, long)]
    branch: Option<String>,
}

fn get_commit_times_by_author(
    args: &Args,
    repo: &gix::Repository,
) -> anyhow::Result<HashMap<BString, Vec<gix::date::Time>>> {
    let refs = repo.references()?;
    let prefix = if let Some(branch) = &args.branch {
        format!("refs/heads/{branch}")
    } else {
        "refs/heads/".to_string()
    };
    let heads = refs.prefixed(prefix.as_str())?;

    let mut visited = HashSet::new();
    let mut times_by_author: HashMap<BString, Vec<gix::date::Time>> = HashMap::new();
    for head in heads.filter_map(|h| h.ok()) {
        let mut stack = vec![head.id()];
        while let Some(id) = stack.pop() {
            let Ok(commit) = repo.find_commit(id) else {
                continue;
            };

            if visited.contains(&commit.id) {
                // This commit and its parents have already been visited. Any further work is
                // redundant.
                continue;
            }
            visited.insert(commit.id);

            let stack_len = stack.len();
            // extend the stack directly to avoid allocating a temporary vec for the parents.
            stack.extend(commit.parent_ids());
            let num_parents = stack.len() - stack_len;

            if let Ok(author) = commit.author()
                && let Ok(time) = author.time()
            {
                let is_merge = num_parents > 1;
                if !is_merge || args.merge_commits {
                    // TODO:
                    // - filter by since/until
                    // - consider using name instead of email (or both?) (or configurable?)
                    // - email/name aliases
                    if let Some(times) = times_by_author.get_mut(author.email) {
                        times.push(time);
                    } else {
                        times_by_author.insert(author.email.into(), vec![time]);
                    }
                }
            }
        }
    }

    for times in times_by_author.values_mut() {
        times.sort();
    }

    Ok(times_by_author)
}

fn estimate_hours(args: &Args, times: &[gix::date::Time]) -> u32 {
    if times.len() < 2 {
        return 0;
    }

    let mut hours = 10f64;

    for window in times.windows(2) {
        let (time, next_time) = (window[0], window[1]);
        let diff_in_minutes = (next_time.seconds - time.seconds) as f64 / 60.0;

        if diff_in_minutes < args.max_commit_diff as f64 {
            hours += diff_in_minutes / 60.0;
        } else {
            hours += args.first_commit_add as f64 / 60.0;
        }
    }

    hours.round() as u32
}

fn main() -> anyhow::Result<()> {
    if std::fs::exists(".git/shallow")? {
        bail!(
            "Cannot analyze shallow copies. Please run `git fetch --unshallow` before continuing."
        );
    }

    let args = Args::parse();
    let repo = gix::open(&args.path)?;

    let mut authors = Vec::new();

    for (author, times) in get_commit_times_by_author(&args, &repo)? {
        authors.push((author, times.len(), estimate_hours(&args, &times)));
    }

    // TODO: make sort configurable (by commits or time)
    authors.sort_by_key(|(_, _, time)| *time);

    for (author, commits, time) in authors {
        println!("{author}: {} commits, {} hours", commits, time);
    }

    Ok(())
}
