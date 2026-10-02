//! Standard output that tolerates a closed pipe. `print!` panics when the reader goes away
//! (`lopi list | head -1`); these return the error instead, and `main` treats a broken
//! pipe as a quiet, successful exit, like other command-line tools.

use std::io::{self, Write};

pub fn print(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Whether `err` (anywhere in its chain) is "the reader closed the pipe".
pub fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|io| io.kind() == io::ErrorKind::BrokenPipe)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn finds_broken_pipe_in_the_chain() {
        let broken: anyhow::Result<()> = Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        let wrapped = broken.context("cannot print the list").unwrap_err();
        assert!(is_broken_pipe(&wrapped));
        let other = anyhow::anyhow!("something else");
        assert!(!is_broken_pipe(&other));
        // Windows reports a closed pipe as error 109/232; std maps both to BrokenPipe.
        #[cfg(windows)]
        assert!(is_broken_pipe(&io::Error::from_raw_os_error(109).into()));
    }
}
