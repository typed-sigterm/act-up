use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct ParsedUsesLine {
    pub prefix: String,
    pub value_raw: String,
    pub comment_suffix: String,
}

#[derive(Debug)]
pub struct ParsedQuote<'a> {
    pub value: &'a str,
    pub quote: &'a str,
}

#[derive(Debug)]
pub struct RepoRef<'a> {
    pub hostname: &'a str,
    pub owner: &'a str,
    pub repo: &'a str,
    pub current_ref: &'a str,
}

pub fn parse_uses_line(line: &str) -> Option<ParsedUsesLine> {
    let caps = uses_regex().captures(line)?;

    let prefix = caps
        .name("prefix")
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    let rest = caps.name("rest").map(|m| m.as_str()).unwrap_or("");

    if rest.starts_with('#') {
        return None;
    }

    let (value_raw, comment_suffix) = if let Some(idx) = rest.find(" #") {
        (rest[..idx].trim().to_string(), rest[idx..].to_string())
    } else {
        (rest.trim().to_string(), String::new())
    };

    Some(ParsedUsesLine {
        prefix,
        value_raw,
        comment_suffix,
    })
}

pub fn parse_quote(input: &str) -> ParsedQuote<'_> {
    let trimmed = input.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0] as char;
        let last = bytes[bytes.len() - 1] as char;
        if (first == '"' || first == '\'') && first == last {
            return ParsedQuote {
                value: &trimmed[1..trimmed.len() - 1],
                quote: &trimmed[..1],
            };
        }
    }
    ParsedQuote {
        value: trimmed,
        quote: "",
    }
}

pub fn parse_repo_ref(input: &str) -> Option<RepoRef<'_>> {
    if input.starts_with("docker://") || input.starts_with("./") || input.starts_with("../") {
        return None;
    }

    let caps = repo_regex().captures(input)?;
    let hostname = caps
        .name("hostname")
        .map(|m| m.as_str())
        .unwrap_or("github.com");
    let owner = caps.name("owner")?.as_str();
    let repo = caps.name("repo")?.as_str();
    let current_ref = caps.name("ref")?.as_str();

    Some(RepoRef {
        hostname,
        owner,
        repo,
        current_ref,
    })
}

pub fn is_sha(value: &str) -> bool {
    let len = value.len();
    (len == 40 || len == 64) && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_short_sha(value: &str) -> bool {
    (value.len() == 6 || value.len() == 7) && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_version_like_ref(value: &str) -> bool {
    version_like_regex().is_match(value)
}

pub fn parse_comment_ref(comment_body: &str) -> Option<String> {
    let trimmed = comment_body.trim();
    if trimmed.is_empty() || trimmed == "ratchet:exclude" {
        return None;
    }

    if let Some(caps) = pin_token_regex().captures(comment_body) {
        return caps.name("version").map(|m| m.as_str().to_string());
    }

    bare_token_regex()
        .captures(comment_body)
        .and_then(|c| c.name("token").map(|m| m.as_str().to_string()))
}

pub fn replace_ref(value: &str, new_ref: &str) -> String {
    if let Some(idx) = value.rfind('@') {
        format!("{}@{}", &value[..idx], new_ref)
    } else {
        value.to_string()
    }
}

fn uses_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<prefix>\s+(?:-\s+)?uses\s*:\s*)(?P<rest>.+)$").expect("valid uses regex")
    })
}

fn repo_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:https://(?P<hostname>[^/]+)/)?(?P<owner>[^/]+)/(?P<repo>[^/@]+)(?:/(?P<path>.+?))?@(?P<ref>.+)$",
        )
        .expect("valid repo regex")
    })
}

fn pin_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\s*(?:(?:renovate\s*:\s*)?(?:pin\s+|tag\s*=\s*)?|(?:ratchet:[\w-]+/[.\w-]+))?@?(?P<version>([\w-]*[-/])?v?\d+(?:\.\d+(?:\.\d+)?)?)",
        )
        .expect("valid pin token regex")
    })
}

fn bare_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(?P<token>\S+)\s*$").expect("valid bare token regex"))
}

fn version_like_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^v?\d+").expect("valid version-like regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uses_and_comment() {
        let line = "      - uses: \"actions/checkout@1e204e9a9253d643386038d443f96446fa156a97\" # tag=v4.2.0";
        let parsed = parse_uses_line(line).expect("uses line");

        assert_eq!(parsed.prefix, "      - uses: ");
        assert_eq!(
            parsed.value_raw,
            "\"actions/checkout@1e204e9a9253d643386038d443f96446fa156a97\""
        );
        assert_eq!(parsed.comment_suffix, " # tag=v4.2.0");
        assert_eq!(parse_comment_ref("tag=v4.2.0"), Some("v4.2.0".to_string()));
    }

    #[test]
    fn parse_repository_ref() {
        let q = parse_quote("'autofix-ci/action@635ffb0c9798bd160680f18fd73371e355b85f27'");
        let parsed = parse_repo_ref(q.value).expect("repo ref");

        assert_eq!(parsed.owner, "autofix-ci");
        assert_eq!(parsed.repo, "action");
        assert_eq!(parsed.hostname, "github.com");
        assert!(is_sha(parsed.current_ref));
    }

    #[test]
    fn parse_bare_non_semver_ref_comment() {
        assert_eq!(
            parse_comment_ref(" cargo-llvm-cov"),
            Some("cargo-llvm-cov".to_string())
        );
    }
}
