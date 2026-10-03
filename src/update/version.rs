//! Release versions: exactly `X.Y.Z`, as lopi's releases are numbered. Pure.

use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow};

/// `major.minor.patch`; compares field by field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// The version of this build.
    pub fn current() -> Self {
        env!("CARGO_PKG_VERSION")
            .parse()
            .expect("the package version is X.Y.Z")
    }
}

impl FromStr for Version {
    type Err = anyhow::Error;

    /// Strict: three plain numbers, no `v`, no pre-release or build suffix.
    fn from_str(text: &str) -> Result<Self> {
        let bad = || anyhow!("'{text}' is not a version like 1.2.3");
        let mut parts = text.split('.').map(|part| {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            part.parse::<u64>().map_err(|_| bad())
        });
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        Ok(Self(major?, minor?, patch?))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_versions_only() {
        assert_eq!("0.4.0".parse::<Version>().unwrap(), Version(0, 4, 0));
        assert_eq!("10.20.30".parse::<Version>().unwrap(), Version(10, 20, 30));
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3-rc.1",
            "1.2.3+b",
            "1..3",
            " 1.2.3",
            "1.-2.3",
            "1.2.x",
            "+1.2.3",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }

    #[test]
    fn orders_numerically() {
        let v = |text: &str| text.parse::<Version>().unwrap();
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.4.1") > v("0.4.0"));
        assert_eq!(v("0.4.0").to_string(), "0.4.0");
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
