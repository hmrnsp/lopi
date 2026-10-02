//! A marked block of lines in a shell profile, so lopi can add its setup once and
//! remove exactly what it added.

const START: &str = "# >>> lopi >>>";
const END: &str = "# <<< lopi <<<";

/// `text` with the block appended, or `None` if a block is already there. Uses the line
/// ending the file already uses.
pub fn add_block(text: &str, body: &str) -> Option<String> {
    if text.contains(START) {
        return None;
    }
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(nl);
    }
    for line in [START, body, END] {
        out.push_str(line);
        out.push_str(nl);
    }
    Some(out)
}

/// `text` without the block (start line through end line), or `Ok(None)` if there is no
/// block. A start marker without an end marker is an error: guessing could delete the
/// user's own lines.
pub fn remove_block(text: &str) -> Result<Option<String>, String> {
    let Some(start) = text.find(START) else {
        return Ok(None);
    };
    let Some(end_marker) = text[start..].find(END).map(|i| start + i) else {
        return Err(format!("found `{START}` without `{END}`"));
    };
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[end_marker..]
        .find('\n')
        .map_or(text.len(), |i| end_marker + i + 1);
    Ok(Some(format!(
        "{}{}",
        &text[..line_start],
        &text[line_end..]
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "setup line";

    #[test]
    fn add_to_empty_and_existing_files() {
        assert_eq!(
            add_block("", BODY).unwrap(),
            "# >>> lopi >>>\nsetup line\n# <<< lopi <<<\n"
        );
        assert_eq!(
            add_block("Set-Alias ll ls", BODY).unwrap(),
            "Set-Alias ll ls\n# >>> lopi >>>\nsetup line\n# <<< lopi <<<\n"
        );
        assert_eq!(
            add_block("a\r\n", BODY).unwrap(),
            "a\r\n# >>> lopi >>>\r\nsetup line\r\n# <<< lopi <<<\r\n"
        );
    }

    #[test]
    fn add_is_idempotent_and_remove_restores() {
        for original in ["", "a\n", "a\r\nb\r\n", "no newline at end"] {
            let added = add_block(original, BODY).unwrap();
            assert_eq!(add_block(&added, BODY), None);
            let removed = remove_block(&added).unwrap().unwrap();
            let expected = if original.is_empty() || original.ends_with('\n') {
                original.to_string()
            } else {
                format!("{original}\n")
            };
            assert_eq!(removed, expected, "{original:?}");
        }
    }

    #[test]
    fn remove_keeps_surrounding_lines() {
        let text = "before\n# >>> lopi >>>\nsetup line\n# <<< lopi <<<\nafter\n";
        assert_eq!(remove_block(text).unwrap().unwrap(), "before\nafter\n");
        assert_eq!(remove_block("nothing here\n").unwrap(), None);
        assert!(remove_block("# >>> lopi >>>\nmine\n").is_err());
    }
}
