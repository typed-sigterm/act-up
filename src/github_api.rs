use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;
use reqwest::blocking::Client;
use serde::Deserialize;
use tracing::debug;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefPrecision {
    Major,
    Minor,
    Patch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedSemverRef {
    prefix: String,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    precision: RefPrecision,
}

#[derive(Debug, Deserialize)]
struct CommitResponse {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct TagItem {
    name: String,
}

pub struct GithubApi {
    client: Client,
    commit_cache: HashMap<(String, String, String), String>,
    tags_cache: HashMap<(String, String), Vec<String>>,
}

impl GithubApi {
    pub fn new() -> Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("act-up/0.1"),
        );

        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            let value = format!("Bearer {token}");
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&value)
                    .context("invalid GITHUB_TOKEN for Authorization header")?,
            );
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build github http client")?;

        Ok(Self {
            client,
            commit_cache: HashMap::new(),
            tags_cache: HashMap::new(),
        })
    }

    pub fn resolve_ref_sha(&mut self, owner: &str, repo: &str, ref_name: &str) -> Result<String> {
        let key = (owner.to_string(), repo.to_string(), ref_name.to_string());
        if let Some(found) = self.commit_cache.get(&key) {
            return Ok(found.clone());
        }

        let url = format!("https://api.github.com/repos/{owner}/{repo}/commits/{ref_name}");
        debug!(owner, repo, ref_name, "resolving ref to commit sha");

        let response = self
            .client
            .get(url)
            .send()
            .with_context(|| format!("failed to resolve {owner}/{repo} ref {ref_name}"))?
            .error_for_status()
            .with_context(|| {
                format!("github API returned error for {owner}/{repo} ref {ref_name}")
            })?;

        let payload: CommitResponse = response.json().with_context(|| {
            format!("failed parsing github response for {owner}/{repo} ref {ref_name}")
        })?;

        self.commit_cache.insert(key, payload.sha.clone());
        Ok(payload.sha)
    }

    pub fn resolve_updated_ref(
        &mut self,
        owner: &str,
        repo: &str,
        current_ref: &str,
    ) -> Result<Option<String>> {
        let Some(current) = parse_semverish_ref(current_ref) else {
            // Branch-like refs keep same textual ref under "preserve original format" policy.
            return Ok(None);
        };

        let tags = self.list_repo_tags(owner, repo)?;
        let mut best: Option<(u64, u64, u64)> = None;

        for tag in tags {
            let Some(parsed_tag) = parse_semverish_ref(&tag) else {
                continue;
            };
            if parsed_tag.prefix != current.prefix {
                continue;
            }

            let t_major = parsed_tag.major;
            let t_minor = parsed_tag.minor;
            let t_patch = parsed_tag.patch;

            let in_scope = if current.precision == RefPrecision::Major {
                t_major == current.major
            } else {
                t_major == current.major && t_minor == current.minor
            };

            if !in_scope {
                continue;
            }

            let score = (t_major, t_minor.unwrap_or(0), t_patch.unwrap_or(0));
            if let Some(current_best) = &best {
                if score.0 > current_best.0
                    || (score.0 == current_best.0 && score.1 > current_best.1)
                    || (score.0 == current_best.0
                        && score.1 == current_best.1
                        && score.2 > current_best.2)
                {
                    best = Some(score);
                }
            } else {
                best = Some(score);
            }
        }

        let Some(best_score) = best else {
            return Ok(None);
        };

        let candidate = format_ref_with_style(&current, best_score.0, best_score.1, best_score.2);
        if candidate == current_ref {
            Ok(None)
        } else {
            Ok(Some(candidate))
        }
    }

    fn list_repo_tags(&mut self, owner: &str, repo: &str) -> Result<Vec<String>> {
        let key = (owner.to_string(), repo.to_string());
        if let Some(tags) = self.tags_cache.get(&key) {
            return Ok(tags.clone());
        }

        let url = format!("https://api.github.com/repos/{owner}/{repo}/tags?per_page=100");
        debug!(owner, repo, "listing repository tags");

        let response = self
            .client
            .get(url)
            .send()
            .with_context(|| format!("failed listing tags for {owner}/{repo}"))?
            .error_for_status()
            .with_context(|| {
                format!("github API returned error while listing tags for {owner}/{repo}")
            })?;

        let tags: Vec<TagItem> = response
            .json()
            .with_context(|| format!("failed parsing tag list for {owner}/{repo}"))?;
        let tag_names: Vec<String> = tags.into_iter().map(|t| t.name).collect();

        self.tags_cache.insert(key, tag_names.clone());
        Ok(tag_names)
    }
}

fn parse_semverish_ref(input: &str) -> Option<ParsedSemverRef> {
    let caps = semverish_regex().captures(input)?;

    let mut prefix = caps
        .name("prefix")
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    if caps.name("v").is_some() {
        prefix.push('v');
    }

    let major = caps.name("major")?.as_str().parse::<u64>().ok()?;
    let minor = caps
        .name("minor")
        .and_then(|m| m.as_str().parse::<u64>().ok());
    let patch = caps
        .name("patch")
        .and_then(|m| m.as_str().parse::<u64>().ok());

    let precision = if patch.is_some() {
        RefPrecision::Patch
    } else if minor.is_some() {
        RefPrecision::Minor
    } else {
        RefPrecision::Major
    };

    Some(ParsedSemverRef {
        prefix,
        major,
        minor,
        patch,
        precision,
    })
}

fn format_ref_with_style(
    current: &ParsedSemverRef,
    best_major: u64,
    best_minor: u64,
    best_patch: u64,
) -> String {
    match current.precision {
        RefPrecision::Major => format!("{}{best_major}", current.prefix),
        RefPrecision::Minor => format!("{}{best_major}.{best_minor}", current.prefix),
        RefPrecision::Patch => format!("{}{best_major}.{best_minor}.{best_patch}", current.prefix),
    }
}

fn semverish_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?P<prefix>[\w-]*[-/])?(?P<v>v)?(?P<major>\d+)(?:\.(?P<minor>\d+))?(?:\.(?P<patch>\d+))?$",
        )
        .expect("valid semverish regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_semverish_refs() {
        assert_eq!(
            parse_semverish_ref("v4"),
            Some(ParsedSemverRef {
                prefix: "v".to_string(),
                major: 4,
                minor: None,
                patch: None,
                precision: RefPrecision::Major,
            })
        );
        assert_eq!(
            parse_semverish_ref("v1.2"),
            Some(ParsedSemverRef {
                prefix: "v".to_string(),
                major: 1,
                minor: Some(2),
                patch: None,
                precision: RefPrecision::Minor,
            })
        );
        assert_eq!(
            parse_semverish_ref("prefix/v1.2.3"),
            Some(ParsedSemverRef {
                prefix: "prefix/v".to_string(),
                major: 1,
                minor: Some(2),
                patch: Some(3),
                precision: RefPrecision::Patch,
            })
        );
        assert_eq!(parse_semverish_ref("main"), None);
    }

    #[test]
    fn preserves_precision_style() {
        let major = ParsedSemverRef {
            prefix: "v".to_string(),
            major: 6,
            minor: None,
            patch: None,
            precision: RefPrecision::Major,
        };
        let minor = ParsedSemverRef {
            prefix: "v".to_string(),
            major: 1,
            minor: Some(2),
            patch: None,
            precision: RefPrecision::Minor,
        };
        let patch = ParsedSemverRef {
            prefix: "v".to_string(),
            major: 1,
            minor: Some(2),
            patch: Some(3),
            precision: RefPrecision::Patch,
        };

        assert_eq!(format_ref_with_style(&major, 6, 1, 4), "v6");
        assert_eq!(format_ref_with_style(&minor, 1, 2, 9), "v1.2");
        assert_eq!(format_ref_with_style(&patch, 1, 2, 9), "v1.2.9");
    }
}
