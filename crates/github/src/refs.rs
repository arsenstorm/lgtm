//! Naming a repository or an issue: the URL and shorthand forms accepted.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub repo: String,
}

/// Parses `https://github.com/o/r`, `https://github.com/o/r.git`, and
/// `git@github.com:o/r.git`. Anything else, including other hosts, is `None`.
pub fn parse_repo(url: &str) -> Option<Repo> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    Repo::parse(rest.strip_suffix(".git").unwrap_or(rest))
}

impl Repo {
    fn parse(s: &str) -> Option<Self> {
        let (owner, repo) = s.split_once('/')?;
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        Some(Repo {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }
}

/// Parses `https://github.com/o/r/issues/N`, `o/r#N`, and `github:o/r#N`.
pub fn parse_issue(s: &str) -> Option<(Repo, u64)> {
    let (repo, number) = match s.strip_prefix("https://github.com/") {
        Some(rest) => rest.split_once("/issues/")?,
        None => s.strip_prefix("github:").unwrap_or(s).split_once('#')?,
    };
    Some((Repo::parse(repo)?, number.parse().ok()?))
}
