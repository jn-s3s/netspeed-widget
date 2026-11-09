//! GitHub release version checking and URL opening.
//!
//! Compares the running package version against the latest release from the
//! GitHub Releases API (`https://api.github.com/repos/jn-s3s/netspeed-widget/releases/latest`).
//! When a newer version is published, callers can display the tag and open its
//! release page in the user's default browser.

use std::cmp::Ordering;
use std::fmt;
use std::io::Read;
use std::time::Duration;

use serde::Deserialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::logger;
use crate::win::gdi::to_wide;

/// API endpoint returning the latest published GitHub release.
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/jn-s3s/netspeed-widget/releases/latest";

/// Fallback web page for the repository's releases.
pub const RELEASES_PAGE_URL: &str = "https://github.com/jn-s3s/netspeed-widget/releases";

/// Trusted URL prefix for opening a specific release tag page in the browser.
const ALLOWED_TAG_URL_PREFIX: &str = "https://github.com/jn-s3s/netspeed-widget/releases/tag/";

/// Maximum bytes read from a GitHub release response body.
const MAX_RELEASE_BODY_BYTES: usize = 256 * 1024;

/// Parsed semantic version (`major.minor.patch` with optional pre-release).
#[derive(Clone, Debug, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl SemVer {
    /// Parses a version string or release tag (with optional leading `v` or `V`).
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let stripped = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        // Ignore build metadata (+...) for version precedence.
        let without_build = stripped.split('+').next()?;
        let (core, pre) = match without_build.split_once('-') {
            Some((core, pre)) => {
                if !is_valid_prerelease(pre) {
                    return None;
                }
                (core, Some(pre.to_string()))
            }
            None => (without_build, None),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

fn is_valid_prerelease(pre: &str) -> bool {
    if pre.is_empty() {
        return false;
    }
    for segment in pre.split('.') {
        if segment.is_empty()
            || !segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return false;
        }
        if segment.bytes().all(|b| b.is_ascii_digit())
            && segment.len() > 1
            && segment.starts_with('0')
        {
            return false;
        }
    }
    true
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.pre {
            Some(pre) => write!(f, "{}.{}.{}-{pre}", self.major, self.minor, self.patch),
            None => write!(f, "{}.{}.{}", self.major, self.minor, self.patch),
        }
    }
}

impl PartialEq for SemVer {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            Ordering::Equal => match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A normal release has higher precedence than a pre-release.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => compare_prerelease(a, b),
            },
            ord => ord,
        }
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(a: &str, b: &str) -> Ordering {
    let mut a_parts = a.split('.');
    let mut b_parts = b.split('.');
    loop {
        match (a_parts.next(), b_parts.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a_id), Some(b_id)) => {
                let ord = match (a_id.parse::<u64>(), b_id.parse::<u64>()) {
                    (Ok(a_num), Ok(b_num)) => a_num.cmp(&b_num),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => a_id.cmp(b_id),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

/// Information about a newer release available on GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    /// Clean version string without a leading `v`, e.g. `"2.2.0"`.
    pub version: String,
    /// Validated HTTPS URL to the GitHub release page.
    pub url: String,
}

/// Outcome of comparing the running version with the latest GitHub release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheck {
    /// The running version is at or ahead of the latest published release.
    UpToDate,
    /// A newer version is available on GitHub.
    Available(UpdateInfo),
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Queries the GitHub Releases API and compares the latest release with the running version.
///
/// # Errors
/// Returns an error if the request fails, the JSON payload is invalid, or the
/// release tag cannot be parsed as a semantic version.
pub fn check_for_update() -> Result<UpdateCheck, String> {
    let current = env!("CARGO_PKG_VERSION");
    logger::info(
        "update",
        &format!("checking for updates (current v{current})"),
    );
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(15))
        .timeout(Duration::from_secs(20))
        .build();
    let user_agent = format!("{}/{}", env!("CARGO_PKG_NAME"), current);
    let response = agent
        .get(LATEST_RELEASE_API_URL)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &user_agent)
        .call()
        .map_err(|err| format!("release query failed: {err}"))?;
    let mut body = String::new();
    response
        .into_reader()
        .take((MAX_RELEASE_BODY_BYTES + 1) as u64)
        .read_to_string(&mut body)
        .map_err(|err| format!("could not read release response: {err}"))?;
    if body.len() > MAX_RELEASE_BODY_BYTES {
        return Err("release response exceeded size limit".to_string());
    }
    evaluate_release_json(current, &body)
}

/// Parses a GitHub release JSON payload and compares it against `current_version`.
///
/// Draft and pre-release entries are treated as [`UpdateCheck::UpToDate`].
/// If `html_url` does not point to the repository's GitHub releases page,
/// [`RELEASES_PAGE_URL`] is used instead.
///
/// # Errors
/// Returns an error if `current_version` or `tag_name` is not valid SemVer, or
/// if `json` is not a valid release object.
pub fn evaluate_release_json(current_version: &str, json: &str) -> Result<UpdateCheck, String> {
    let current = SemVer::parse(current_version)
        .ok_or_else(|| format!("invalid current version '{current_version}'"))?;
    let release: GithubRelease =
        serde_json::from_str(json).map_err(|err| format!("invalid release JSON payload: {err}"))?;
    if release.draft || release.prerelease {
        return Ok(UpdateCheck::UpToDate);
    }
    let latest = SemVer::parse(&release.tag_name)
        .ok_or_else(|| format!("invalid release tag '{}'", release.tag_name))?;
    if latest.pre.is_some() {
        return Ok(UpdateCheck::UpToDate);
    }
    if latest > current {
        let url = sanitize_release_url(&release.html_url);
        Ok(UpdateCheck::Available(UpdateInfo {
            version: latest.to_string(),
            url,
        }))
    } else {
        Ok(UpdateCheck::UpToDate)
    }
}

/// Ensures the release URL points to the repository's HTTPS GitHub releases page.
pub fn sanitize_release_url(candidate: &str) -> String {
    let trimmed = candidate.trim();
    if trimmed == RELEASES_PAGE_URL {
        return trimmed.to_string();
    }
    if let Some(tag) = trimmed.strip_prefix(ALLOWED_TAG_URL_PREFIX) {
        if !tag.is_empty()
            && tag != "."
            && tag != ".."
            && !tag.contains("..")
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
        {
            return trimmed.to_string();
        }
    }
    RELEASES_PAGE_URL.to_string()
}

/// Opens a validated release URL in the user's default browser.
///
/// # Errors
/// Returns an error if the shell refuses to launch the URL.
pub fn open_release_url(url: &str) -> Result<(), String> {
    let safe_url = sanitize_release_url(url);
    let operation = to_wide("open");
    let target = to_wide(&safe_url);
    // SAFETY: Both UTF-16 strings are NUL-terminated and live across the call;
    // no parent window or directory borrow is needed.
    let result = unsafe {
        ShellExecuteW(
            HWND::default(),
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(format!(
            "ShellExecuteW failed with status code {}",
            result.0 as isize
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parses_tags_and_compares_versions() {
        let v2_1_0 = SemVer::parse("v2.1.0").unwrap();
        let v2_1_0_plain = SemVer::parse("2.1.0").unwrap();
        let v2_1_1 = SemVer::parse("v2.1.1").unwrap();
        let v2_2_0 = SemVer::parse("V2.2.0").unwrap();
        let v3_0_0 = SemVer::parse("v3.0.0+build.123").unwrap();
        let v2_2_0_rc1 = SemVer::parse("v2.2.0-rc.1").unwrap();
        let v2_2_0_rc2 = SemVer::parse("v2.2.0-rc.2").unwrap();

        assert_eq!(v2_1_0, v2_1_0_plain);
        assert!(v2_1_1 > v2_1_0);
        assert!(v2_2_0 > v2_1_1);
        assert!(v3_0_0 > v2_2_0);
        assert!(v2_2_0 > v2_2_0_rc2);
        assert!(v2_2_0_rc2 > v2_2_0_rc1);
        assert!(v2_2_0_rc1 > v2_1_1);

        assert!(SemVer::parse("").is_none());
        assert!(SemVer::parse("v2.1").is_none());
        assert!(SemVer::parse("v2.1.0.1").is_none());
        assert!(SemVer::parse("v2.1.0-").is_none());
        assert!(SemVer::parse("v2.1.0-rc..1").is_none());
        assert!(SemVer::parse("v2.1.0-01").is_none());
        assert!(SemVer::parse("v2.1.0-rc.1\nhttps://evil").is_none());
        assert!(SemVer::parse("latest").is_none());
    }

    #[test]
    fn release_evaluation_detects_newer_versions_and_ignores_older_or_drafts() {
        let newer_json = r#"{
            "tag_name": "v2.2.0",
            "html_url": "https://github.com/jn-s3s/netspeed-widget/releases/tag/v2.2.0",
            "draft": false,
            "prerelease": false
        }"#;
        assert_eq!(
            evaluate_release_json("2.1.0", newer_json).unwrap(),
            UpdateCheck::Available(UpdateInfo {
                version: "2.2.0".to_string(),
                url: "https://github.com/jn-s3s/netspeed-widget/releases/tag/v2.2.0".to_string(),
            })
        );
        assert_eq!(
            evaluate_release_json("2.2.0", newer_json).unwrap(),
            UpdateCheck::UpToDate
        );
        assert_eq!(
            evaluate_release_json("2.3.0", newer_json).unwrap(),
            UpdateCheck::UpToDate
        );

        let draft_json = r#"{
            "tag_name": "v9.0.0",
            "html_url": "https://github.com/jn-s3s/netspeed-widget/releases/tag/v9.0.0",
            "draft": true,
            "prerelease": false
        }"#;
        assert_eq!(
            evaluate_release_json("2.1.0", draft_json).unwrap(),
            UpdateCheck::UpToDate
        );

        let prerelease_json = r#"{
            "tag_name": "v9.0.0-rc.1",
            "html_url": "https://github.com/jn-s3s/netspeed-widget/releases/tag/v9.0.0-rc.1",
            "draft": false,
            "prerelease": true
        }"#;
        assert_eq!(
            evaluate_release_json("2.1.0", prerelease_json).unwrap(),
            UpdateCheck::UpToDate
        );

        let unflagged_prerelease_json = r#"{
            "tag_name": "v9.0.0-rc.1",
            "html_url": "https://github.com/jn-s3s/netspeed-widget/releases/tag/v9.0.0-rc.1",
            "draft": false,
            "prerelease": false
        }"#;
        assert_eq!(
            evaluate_release_json("2.1.0", unflagged_prerelease_json).unwrap(),
            UpdateCheck::UpToDate
        );
    }

    #[test]
    fn untrusted_release_urls_fall_back_to_repository_releases_page() {
        let untrusted_json = r#"{
            "tag_name": "v2.2.0",
            "html_url": "https://example.com/malicious",
            "draft": false,
            "prerelease": false
        }"#;
        assert_eq!(
            evaluate_release_json("2.1.0", untrusted_json).unwrap(),
            UpdateCheck::Available(UpdateInfo {
                version: "2.2.0".to_string(),
                url: RELEASES_PAGE_URL.to_string(),
            })
        );
        assert_eq!(
            sanitize_release_url("file:///C:/Windows/System32/calc.exe"),
            RELEASES_PAGE_URL
        );
        assert_eq!(
            sanitize_release_url(
                "https://github.com/jn-s3s/netspeed-widget/releases/tag/v2.1.0\nhttps://evil"
            ),
            RELEASES_PAGE_URL
        );
        assert_eq!(
            sanitize_release_url(
                "https://github.com/jn-s3s/netspeed-widget/releases/../../../evil/repo"
            ),
            RELEASES_PAGE_URL
        );
        assert_eq!(
            sanitize_release_url("https://github.com/jn-s3s/netspeed-widget/releases-evil"),
            RELEASES_PAGE_URL
        );
        assert_eq!(
            sanitize_release_url(
                "https://github.com/jn-s3s/netspeed-widget/releases/download/v2.2.0/payload.exe"
            ),
            RELEASES_PAGE_URL
        );
        assert_eq!(sanitize_release_url(RELEASES_PAGE_URL), RELEASES_PAGE_URL);
    }
}
