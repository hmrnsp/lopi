use std::fmt;

/// Errors from looking up a profile by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    NotFound {
        query: String,
        /// Names worth suggesting: prefix matches (when only a full name is accepted)
        /// or names that look like a typo of `query`.
        suggestions: Vec<String>,
        /// The command accepts full names only (`rm`, `edit`), not prefixes.
        full_name_required: bool,
    },
    /// The query matched more than one profile.
    Ambiguous {
        query: String,
        candidates: Vec<String>,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound {
                query,
                suggestions,
                full_name_required,
            } => {
                if *full_name_required && !suggestions.is_empty() {
                    write!(
                        f,
                        "no profile named exactly '{query}'; this command needs the full name"
                    )?;
                } else {
                    write!(f, "no profile named '{query}'")?;
                }
                match suggestions.as_slice() {
                    [] => write!(f, " (see `lopi list`)"),
                    [one] => write!(f, "; did you mean '{one}'?"),
                    many => write!(f, "; did you mean one of: {}?", many.join(", ")),
                }
            }
            Self::Ambiguous { query, candidates } => write!(
                f,
                "'{query}' matches several profiles: {}; type more of the name",
                candidates.join(", ")
            ),
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
