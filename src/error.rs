use std::fmt;

/// Errors from looking up a profile by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No profile has exactly this name. Near misses are suggested, never used.
    NotFound {
        query: String,
        /// Names worth suggesting, of the kind given by `near_miss`.
        suggestions: Vec<String>,
        near_miss: NearMiss,
    },
}

/// Why the suggestions of a [`ResolveError::NotFound`] were picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NearMiss {
    /// Nothing close.
    None,
    /// The same name in another letter case.
    Case,
    /// Names that start with the query: it was typed short.
    Prefix,
    /// Names that look like a typo of the query.
    Typo,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound {
                query,
                suggestions,
                near_miss,
            } => {
                write!(f, "no profile named '{query}'")?;
                let lead = match near_miss {
                    NearMiss::Case => "; names are case-sensitive: did you mean",
                    NearMiss::Prefix => "; type the full name: did you mean",
                    NearMiss::Typo | NearMiss::None => "; did you mean",
                };
                match suggestions.as_slice() {
                    [] => write!(f, " (see `lopi list`)"),
                    [one] => write!(f, "{lead} '{one}'?"),
                    many => write!(f, "{lead} one of: {}?", many.join(", ")),
                }
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// The user stopped an interactive prompt. Not a failure: `main` prints a short note and
/// exits with [`Abort::exit_code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abort {
    /// Esc, or "no" to a confirmation.
    Cancelled,
    /// Ctrl+C.
    Interrupted,
}

impl Abort {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Cancelled => 1,
            // The shell convention for "terminated by SIGINT" (128 + 2).
            Self::Interrupted => 130,
        }
    }
}

impl fmt::Display for Abort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled; nothing was changed"),
            Self::Interrupted => write!(f, "interrupted; nothing was changed"),
        }
    }
}

impl std::error::Error for Abort {}
