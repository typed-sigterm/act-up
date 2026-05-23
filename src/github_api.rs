use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use tracing::debug;

use crate::parser::{RefPrecision, format_ref_with_style, parse_semverish_ref};

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
            reqwest::header::HeaderValue::from_static(concat!(
                "act-up/",
                env!("CARGO_PKG_VERSION")
            )),
        );

        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .context("invalid GITHUB_TOKEN for Authorization header")?,
            );
        }

        let client = Client::builder()
            .default_headers(headers)
            .http1_only()
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

            let in_scope = if current.precision == RefPrecision::Major {
                parsed_tag.major == current.major
            } else {
                parsed_tag.major == current.major && parsed_tag.minor == current.minor
            };

            if in_scope {
                let score = (
                    parsed_tag.major,
                    parsed_tag.minor.unwrap_or(0),
                    parsed_tag.patch.unwrap_or(0),
                );
                best = Some(best.map_or(score, |b| b.max(score)));
            }
        }

        let Some(best_score) = best else {
            return Ok(None);
        };

        let candidate = format_ref_with_style(&current, best_score.0, best_score.1, best_score.2);
        Ok((candidate != current_ref).then_some(candidate))
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
