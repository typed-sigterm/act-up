use std::sync::OnceLock;

use regex::Regex;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefPrecision {
    Major,
    Minor,
    Patch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedSemverRef {
    pub prefix: String,
    pub major: u64,
    pub minor: Option<u64>,
    pub patch: Option<u64>,
    pub precision: RefPrecision,
}

pub fn parse_uses_line(line: &str) -> Option<ParsedUsesLine> {
    let caps = uses_regex().captures(line)?;
    let prefix = caps.name("prefix")?.as_str().to_string();
    let rest = caps.name("rest")?.as_str();

    if rest.trim_start().starts_with('#') {
        return None;
    }

    let (value_raw, comment_suffix) = rest
        .find(" #")
        .map(|idx| (rest[..idx].trim().to_string(), rest[idx..].to_string()))
        .unwrap_or_else(|| (rest.trim().to_string(), String::new()));

    Some(ParsedUsesLine {
        prefix,
        value_raw,
        comment_suffix,
    })
}

pub fn parse_quote(input: &str) -> ParsedQuote<'_> {
    let trimmed = input.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return ParsedQuote {
            value: &trimmed[1..trimmed.len() - 1],
            quote: &trimmed[..1],
        };
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
    Some(RepoRef {
        hostname: caps
            .name("hostname")
            .map(|m| m.as_str())
            .unwrap_or("github.com"),
        owner: caps.name("owner")?.as_str(),
        repo: caps.name("repo")?.as_str(),
        current_ref: caps.name("ref")?.as_str(),
    })
}

pub fn is_sha(value: &str) -> bool {
    let len = value.len();
    (len == 40 || len == 64) && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_short_sha(value: &str) -> bool {
    let len = value.len();
    (6..=10).contains(&len) && value.chars().all(|c| c.is_ascii_hexdigit())
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
    value
        .rfind('@')
        .map(|idx| format!("{}@{}", &value[..idx], new_ref))
        .unwrap_or_else(|| value.to_string())
}

pub fn parse_semverish_ref(input: &str) -> Option<ParsedSemverRef> {
    let caps = semverish_regex().captures(input)?;

    let mut prefix = caps
        .name("prefix")
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    if caps.name("v").is_some() {
        prefix.push('v');
    }

    let major = caps.name("major")?.as_str().parse().ok()?;
    let minor = caps.name("minor").and_then(|m| m.as_str().parse().ok());
    let patch = caps.name("patch").and_then(|m| m.as_str().parse().ok());

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

pub fn format_ref_with_style(
    current: &ParsedSemverRef,
    major: u64,
    minor: u64,
    patch: u64,
) -> String {
    match current.precision {
        RefPrecision::Major => format!("{}{major}", current.prefix),
        RefPrecision::Minor => format!("{}{major}.{minor}", current.prefix),
        RefPrecision::Patch => format!("{}{major}.{minor}.{patch}", current.prefix),
    }
}

fn uses_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?P<prefix>\s+(?:-\s+)?uses\s*:\s*)(?P<rest>.+)$").unwrap())
}

fn repo_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:https://(?P<hostname>[^/]+)/)?(?P<owner>[^/]+)/(?P<repo>[^/@]+)(?:/(?P<path>.+?))?@(?P<ref>.+)$",
        ).unwrap()
    })
}

fn pin_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\s*(?:(?:renovate\s*:\s*)?(?:pin\s+|tag\s*=\s*)?|(?:ratchet:[\w-]+/[.\w-]+))?@?(?P<version>([\w-]*[-/])?v?\d+(?:\.\d+(?:\.\d+)?)?)",
        ).unwrap()
    })
}

fn bare_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(?P<token>\S+)\s*$").unwrap())
}

fn version_like_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^v?\d+").unwrap())
}

fn semverish_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?P<prefix>[\w-]*[-/])?(?P<v>v)?(?P<major>\d+)(?:\.(?P<minor>\d+))?(?:\.(?P<patch>\d+))?$",
        ).unwrap()
    })
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
