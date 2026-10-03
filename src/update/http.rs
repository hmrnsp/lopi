//! Downloads from the GitHub releases of lopi, over HTTPS only. Proxies come from the usual
//! environment variables (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`); certificates are checked
//! against the Mozilla root list built into lopi.

use std::env;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use ureq::Agent;

use super::version::Version;

/// Overrides where releases are looked up (used by tests). Plain `http://` is accepted for
/// this machine only (127.0.0.1, localhost, [::1]).
pub const URL_ENV: &str = "LOPI_UPDATE_URL";

const DEFAULT_BASE: &str = "https://github.com/hmrnsp/lopi/releases";

/// Upper bounds for each download, far above the real sizes.
pub const MANIFEST_LIMIT: u64 = 1024 * 1024;
pub const ARCHIVE_LIMIT: u64 = 64 * 1024 * 1024;
pub const CHECKSUM_LIMIT: u64 = 4 * 1024;

/// The releases of lopi: `<base>/latest/download/<file>`, `<base>/download/v<version>/<file>`
/// and `<base>/tag/v<version>`, as on GitHub.
pub struct Releases {
    base: String,
    agent: Agent,
}

impl Releases {
    /// GitHub, or the address in [`URL_ENV`].
    pub fn from_env() -> Result<Self> {
        match env::var(URL_ENV) {
            Ok(base) if !base.is_empty() => Self::new(&base),
            _ => Self::new(DEFAULT_BASE),
        }
    }

    pub fn new(base: &str) -> Result<Self> {
        let base = base.trim_end_matches('/');
        let local_http = match base.strip_prefix("http://") {
            Some(rest) if is_loopback(rest) => true,
            Some(_) => {
                bail!("{URL_ENV} must be an https:// address (http:// only for this machine)")
            }
            None if base.starts_with("https://") => false,
            None => bail!("{URL_ENV} must be an https:// address"),
        };
        let mut config = Agent::config_builder()
            .https_only(!local_http)
            .user_agent(format!("lopi/{}", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(300)));
        if local_http {
            // A test server on this machine; never send that through a proxy.
            config = config.proxy(None);
        }
        Ok(Self {
            base: base.to_string(),
            agent: config.build().new_agent(),
        })
    }

    /// The manifest of the latest release.
    pub fn latest_manifest(&self) -> Result<Vec<u8>> {
        let url = format!("{}/latest/download/dist-manifest.json", self.base);
        self.get(&url, MANIFEST_LIMIT).map_err(|err| match err {
            Failure::NotFound => anyhow!("no published release found at {url}"),
            Failure::Other(err) => err,
        })
    }

    /// A file of the release `version`. Its tag is fixed, so a release published meanwhile
    /// cannot mix into this download.
    pub fn asset(&self, version: Version, name: &str, limit: u64) -> Result<Vec<u8>> {
        let url = format!("{}/download/v{version}/{name}", self.base);
        self.get(&url, limit).map_err(|err| match err {
            Failure::NotFound => anyhow!("the release v{version} has no file {name}"),
            Failure::Other(err) => err,
        })
    }

    /// The release notes page.
    pub fn page(&self, version: Version) -> String {
        format!("{}/tag/v{version}", self.base)
    }

    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, Failure> {
        let fail = |err: ureq::Error| match err {
            ureq::Error::StatusCode(404) => Failure::NotFound,
            err => Failure::Other(explain(err).context(format!("cannot download {url}"))),
        };
        let mut response = self.agent.get(url).call().map_err(fail)?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(fail)
    }
}

enum Failure {
    NotFound,
    Other(anyhow::Error),
}

/// Adds what to try to the errors people can do something about.
fn explain(err: ureq::Error) -> anyhow::Error {
    let hint = match &err {
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            "the secure connection failed; if your network inspects HTTPS with its own \
             certificate, update with the install script instead"
        }
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(_)
        | ureq::Error::Io(_)
        | ureq::Error::ConnectProxyFailed(_) => {
            "check the internet connection, or set HTTPS_PROXY if you need a proxy"
        }
        _ => return err.into(),
    };
    anyhow::Error::new(err).context(hint)
}

/// `host[:port][/path]` names this machine.
fn is_loopback(rest: &str) -> bool {
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().map(|h| format!("[{h}]"))
    } else {
        authority.split(':').next().map(str::to_string)
    };
    matches!(host.as_deref(), Some("127.0.0.1" | "localhost" | "[::1]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_only_for_this_machine() {
        for ok in [
            "https://github.com/hmrnsp/lopi/releases",
            "https://example.com/r/",
            "http://127.0.0.1:8080/r",
            "http://localhost:1/",
            "http://[::1]:9/r",
            "http://127.0.0.1",
        ] {
            assert!(Releases::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://example.com/r",
            "http://127.0.0.1.example.com/r",
            "http://localhost.evil:80/",
            "http://[::2]/",
            "ftp://127.0.0.1/",
            "github.com/hmrnsp/lopi",
            "",
        ] {
            assert!(Releases::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn builds_release_urls() {
        let releases = Releases::new("https://example.com/r/").unwrap();
        assert_eq!(
            releases.page(Version(0, 5, 0)),
            "https://example.com/r/tag/v0.5.0"
        );
    }
}
