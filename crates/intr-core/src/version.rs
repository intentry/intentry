use semver::Version;
use std::fmt;

// ---------------------------------------------------------------------------
// Version newtype
// ---------------------------------------------------------------------------

/// A thin wrapper around [`semver::Version`] with auto-bump helpers.
///
/// All version arithmetic follows [Semantic Versioning 2.0.0](https://semver.org/).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer(pub Version);

impl SemVer {
    /// Parse a version string (e.g. `"1.2.3"`).
    pub fn parse(s: &str) -> Result<Self, semver::Error> {
        Version::parse(s).map(Self)
    }

    /// Return the initial version for a new prompt: `1.0.0`.
    pub fn initial() -> Self {
        Self(Version::new(1, 0, 0))
    }

    /// Increment the patch component: `1.2.3` → `1.2.4`.
    pub fn bump_patch(&self) -> Self {
        let v = &self.0;
        Self(Version::new(v.major, v.minor, v.patch + 1))
    }

    /// Increment the minor component: `1.2.3` → `1.3.0`.
    pub fn bump_minor(&self) -> Self {
        let v = &self.0;
        Self(Version::new(v.major, v.minor + 1, 0))
    }

    /// Increment the major component: `1.2.3` → `2.0.0`.
    pub fn bump_major(&self) -> Self {
        let v = &self.0;
        Self(Version::new(v.major + 1, 0, 0))
    }

    pub fn inner(&self) -> &Version {
        &self.0
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl serde::Serialize for SemVer {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for SemVer {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        SemVer::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// BumpKind
// ---------------------------------------------------------------------------

/// Which part of the semver to increment on a new commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BumpKind {
    /// Backwards-compatible bug fixes — increments patch.
    #[default]
    Patch,
    /// New backwards-compatible features — increments minor.
    Minor,
    /// Breaking changes — increments major.
    Major,
    /// Use the version supplied in the frontmatter verbatim (skip auto-bump).
    Explicit,
}

impl std::fmt::Display for BumpKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Patch    => write!(f, "patch"),
            Self::Minor    => write!(f, "minor"),
            Self::Major    => write!(f, "major"),
            Self::Explicit => write!(f, "explicit"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_patch() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v.bump_patch().to_string(), "1.2.4");
    }

    #[test]
    fn bump_minor() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v.bump_minor().to_string(), "1.3.0");
    }

    #[test]
    fn bump_major() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v.bump_major().to_string(), "2.0.0");
    }

    #[test]
    fn serde_roundtrip() {
        let v = SemVer::parse("3.1.4").unwrap();
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, "\"3.1.4\"");
        let back: SemVer = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}
