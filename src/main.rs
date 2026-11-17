mod time;

use ahash::{AHashMap, AHashSet};
use std::{
    ops::{Bound, RangeBounds},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};
use clap::Parser;
use gix::bstr::BString;
use jiff::{Span, Unit, Zoned};

use crate::time::CommitTimeBound;

/// Estimate time spent on a project from its Git commits
#[derive(Debug, Parser, Clone)]
#[command(version)]
struct Args {
    /// Start a new session when the gap between commits reaches this duration (e.g. "2h" or "PT2H")
    #[arg(short = 'd', long, value_name = "DURATION", default_value = "2h", value_parser = time::parse_unsigned_duration)]
    max_commit_diff: Duration,

    /// Time credited for the first commit of each session (at most --max-commit-diff)
    #[arg(short, long, value_name = "DURATION", default_value = "2h", value_parser = time::parse_unsigned_duration)]
    first_commit_add: Duration,

    /// Include commits with an author time at or after this bound (e.g. `4 days ago` or `2026-11-13T00:00:00[UTC]`)
    #[arg(long, value_name = "TIME", value_parser = CommitTimeBound::parse)]
    since: Option<CommitTimeBound>,

    /// Include commits with an author time at or before this bound (e.g. `1 day ago` or `2026-11-17T00:00:00[UTC]`)
    #[arg(long, value_name = "TIME", value_parser = CommitTimeBound::parse)]
    until: Option<CommitTimeBound>,

    // TODO: consider making flag instead of value
    /// Include merge commits (enabled by default)
    #[arg(short, long, default_value_t = true)]
    merge_commits: bool,

    /// Path to the Git repository
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    // Aliases of emails for grouping the same activity as one person
    // #[arg(short, long)]
    // email_aliases: HashMap<String, String>,
    /// Local branch name or prefix (all local branches by default)
    #[arg(short, long)]
    branch: Option<String>,
}

impl Args {
    fn commit_time_range(&self) -> anyhow::Result<(Bound<Zoned>, Bound<Zoned>)> {
        if self.since.is_none() && self.until.is_none() {
            return Ok((Bound::Unbounded, Bound::Unbounded));
        }

        let now = Zoned::now();
        let since = self
            .since
            .as_ref()
            .map(|bound| bound.resolve(&now))
            .transpose()
            .context("could not resolve --since")?;
        let until = self
            .until
            .as_ref()
            .map(|bound| bound.resolve(&now))
            .transpose()
            .context("could not resolve --until")?;
        if let (Some(since), Some(until)) = (&since, &until)
            && since > until
        {
            bail!("--since must be at or before --until");
        }

        Ok((
            since.map_or(Bound::Unbounded, Bound::Included),
            until.map_or(Bound::Unbounded, Bound::Included),
        ))
    }
}

fn get_commit_times_by_author(args: &Args) -> anyhow::Result<AHashMap<BString, Vec<Zoned>>> {
    let repo = gix::open(&args.path)?;
    let time_range = args.commit_time_range()?;
    let refs = repo.references()?;
    let prefix = format!("refs/heads/{}", args.branch.as_deref().unwrap_or(""));
    let heads = refs.prefixed(prefix.as_str())?;

    let mut visited = AHashSet::new();
    let mut times_by_author: AHashMap<BString, Vec<Zoned>> = AHashMap::new();
    let mut stack = Vec::new();
    for head in heads.filter_map(|h| h.ok()) {
        stack.clear();
        stack.push(head.id());
        while let Some(id) = stack.pop() {
            if visited.contains(&id.detach()) {
                // This commit and its parents have already been visited. Any further work is
                // redundant.
                continue;
            }
            let Ok(commit) = repo.find_commit(id) else {
                continue;
            };
            visited.insert(commit.id);

            let is_merge = {
                let len = stack.len();
                // extend the stack directly to avoid allocating a temporary vec for the parents.
                stack.extend(commit.parent_ids());
                stack.len() - len > 1
            };
            if is_merge && !args.merge_commits {
                continue;
            }

            let author = commit.author()?;
            let time = author.time()?.to_zoned()?;
            if time_range.contains(&time) {
                // TODO:
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

    for times in times_by_author.values_mut() {
        times.sort();
    }

    Ok(times_by_author)
}

fn estimate_duration(args: &Args, times: &[Zoned]) -> Duration {
    if times.is_empty() {
        return Duration::ZERO;
    }

    let mut total = args.first_commit_add;
    for window in times.windows(2) {
        let gap = window[1].duration_since(&window[0]).unsigned_abs();
        let session_time = if gap < args.max_commit_diff {
            gap
        } else {
            args.first_commit_add
        };
        total = total.saturating_add(session_time);
    }
    total
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.first_commit_add > args.max_commit_diff {
        bail!("--first-commit-add must be at most --max-commit-diff");
    }
    if args.path.join(Path::new(".git/shallow")).try_exists()? {
        bail!(
            "cannot analyze shallow repository at {:?}. Run `git fetch --unshallow` from the repository and try again",
            args.path
        );
    }

    let mut authors = Vec::new();
    for (author, times) in get_commit_times_by_author(&args)? {
        authors.push((author, times.len(), estimate_duration(&args, &times)));
    }

    // TODO: make sort configurable (by commits or time)
    authors.sort_by_key(|(_, _, duration)| *duration);

    let round = jiff::SpanRound::new()
        .smallest(Unit::Minute)
        .increment(5)
        .largest(Unit::Day)
        .days_are_24_hours()
        .mode(jiff::RoundMode::Ceil);
    for (author, commits, duration) in authors {
        println!(
            "{author}: {commits} commits, {:#}",
            Span::try_from(duration)?.round(round)?
        );
    }

    Ok(())
}
