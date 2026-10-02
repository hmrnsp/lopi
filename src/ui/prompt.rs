//! Interactive questions behind a trait, so wizard and picker logic can be tested with a
//! scripted [`Prompter`] instead of a terminal.

use std::rc::Rc;

use anyhow::{Result, anyhow};
use inquire::autocompletion::Replacement;
use inquire::validator::{StringValidator, Validation};
use inquire::{
    Autocomplete, Confirm, CustomUserError, InquireError, Password, PasswordDisplayMode, Select,
    Text,
};
use zeroize::Zeroizing;

use super::table_picker;
use crate::error::Abort;

/// Checks a text answer; `Err` holds the message shown under the input.
pub type Validator = Rc<dyn Fn(&str) -> Result<(), String>>;

/// A free-text question. Empty input means `default` when one is set.
#[derive(Clone, Default)]
pub struct TextQuestion<'a> {
    pub message: &'a str,
    pub default: Option<&'a str>,
    pub help: Option<&'a str>,
    /// Offered while typing (Tab or arrows to pick).
    pub suggestions: Vec<String>,
    pub validate: Option<Validator>,
}

impl<'a> TextQuestion<'a> {
    pub fn new(message: &'a str) -> Self {
        Self {
            message,
            ..Self::default()
        }
    }

    pub fn default_value(mut self, default: Option<&'a str>) -> Self {
        self.default = default;
        self
    }

    pub fn help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }

    pub fn suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }

    pub fn validate(mut self, validate: impl Fn(&str) -> Result<(), String> + 'static) -> Self {
        self.validate = Some(Rc::new(validate));
        self
    }
}

/// Every method fails with [`Abort`] when the user presses Esc or Ctrl+C.
pub trait Prompter {
    /// Index of the chosen option; `start` is the one highlighted at first.
    fn select(&mut self, message: &str, options: &[String], start: usize) -> Result<usize>;
    /// Index of the chosen row of a table; typing filters on every cell.
    fn pick_row(&mut self, message: &str, header: &[&str], rows: &[Vec<String>]) -> Result<usize>;
    fn text(&mut self, question: TextQuestion<'_>) -> Result<String>;
    fn confirm(&mut self, message: &str, default: bool) -> Result<bool>;
    /// Hidden input, never empty. With `confirm`, it must be typed twice. The value is
    /// wiped from memory when dropped.
    fn password(&mut self, message: &str, confirm: bool) -> Result<Zeroizing<String>>;
}

/// The real terminal prompts: inquire, and the ratatui table for [`Prompter::pick_row`].
/// Draws on stderr, so stdout stays clean for scripts.
pub struct TerminalPrompter;

impl Prompter for TerminalPrompter {
    fn select(&mut self, message: &str, options: &[String], start: usize) -> Result<usize> {
        let chosen = Select::new(message, options.to_vec())
            .with_starting_cursor(start.min(options.len().saturating_sub(1)))
            .with_page_size(12)
            .with_help_message("↑↓ to move, type to filter, Enter to choose, Esc to cancel")
            .raw_prompt()
            .map_err(convert)?;
        Ok(chosen.index)
    }

    fn pick_row(&mut self, message: &str, header: &[&str], rows: &[Vec<String>]) -> Result<usize> {
        table_picker::pick_row(message, header, rows)
    }

    fn text(&mut self, question: TextQuestion<'_>) -> Result<String> {
        let mut prompt = Text::new(question.message);
        if let Some(default) = question.default {
            prompt = prompt.with_default(default);
        }
        if let Some(help) = question.help {
            prompt = prompt.with_help_message(help);
        }
        if !question.suggestions.is_empty() {
            prompt = prompt.with_autocomplete(Suggestions(question.suggestions));
        }
        if let Some(validate) = question.validate {
            prompt = prompt.with_validator(ValidatorAdapter(validate));
        }
        prompt.prompt().map_err(convert)
    }

    fn confirm(&mut self, message: &str, default: bool) -> Result<bool> {
        Confirm::new(message)
            .with_default(default)
            .prompt()
            .map_err(convert)
    }

    fn password(&mut self, message: &str, confirm: bool) -> Result<Zeroizing<String>> {
        let not_empty = |input: &str| {
            Ok(if input.is_empty() {
                Validation::Invalid("cannot be empty".into())
            } else {
                Validation::Valid
            })
        };
        let mut prompt = Password::new(message)
            .with_display_mode(PasswordDisplayMode::Hidden)
            .with_validator(not_empty);
        prompt = if confirm {
            prompt.with_custom_confirmation_message("Type it again:")
        } else {
            prompt.without_confirmation()
        };
        prompt.prompt().map(Zeroizing::new).map_err(convert)
    }
}

fn convert(err: InquireError) -> anyhow::Error {
    match err {
        InquireError::OperationCanceled => Abort::Cancelled.into(),
        InquireError::OperationInterrupted => Abort::Interrupted.into(),
        InquireError::NotTTY => anyhow!("this needs an interactive terminal"),
        other => anyhow!(other),
    }
}

#[derive(Clone)]
struct ValidatorAdapter(Validator);

impl StringValidator for ValidatorAdapter {
    fn validate(&self, input: &str) -> Result<Validation, CustomUserError> {
        Ok(match (self.0)(input) {
            Ok(()) => Validation::Valid,
            Err(message) => Validation::Invalid(message.into()),
        })
    }
}

/// Suggestions containing the typed text (ignoring case).
#[derive(Clone)]
struct Suggestions(Vec<String>);

impl Autocomplete for Suggestions {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        let input = input.to_lowercase();
        Ok(self
            .0
            .iter()
            .filter(|s| s.to_lowercase().contains(&input))
            .cloned()
            .collect())
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted: Option<String>,
    ) -> Result<Replacement, CustomUserError> {
        Ok(highlighted)
    }
}

#[cfg(test)]
pub mod scripted {
    //! A [`Prompter`] that replays answers, for tests.

    use std::collections::VecDeque;

    use super::*;

    #[derive(Debug, Clone)]
    pub enum Answer {
        /// Choose the option with this exact text.
        Pick(&'static str),
        Text(&'static str),
        Yes,
        No,
        /// A hidden answer (password or passphrase).
        Secret(&'static str),
        Esc,
        CtrlC,
    }

    #[derive(Default)]
    pub struct Scripted {
        answers: VecDeque<Answer>,
        /// Every question asked, in order (with the options of a select).
        pub asked: Vec<String>,
        /// Validation messages shown for rejected text answers.
        pub rejected: Vec<String>,
    }

    impl Scripted {
        pub fn new(answers: impl IntoIterator<Item = Answer>) -> Self {
            Self {
                answers: answers.into_iter().collect(),
                ..Self::default()
            }
        }

        pub fn finished(&self) -> bool {
            self.answers.is_empty()
        }

        fn next(&mut self, question: &str) -> Result<Answer> {
            match self.answers.pop_front() {
                Some(Answer::Esc) => Err(Abort::Cancelled.into()),
                Some(Answer::CtrlC) => Err(Abort::Interrupted.into()),
                Some(answer) => Ok(answer),
                None => panic!("no scripted answer left for: {question}"),
            }
        }
    }

    impl Prompter for Scripted {
        fn select(&mut self, message: &str, options: &[String], _start: usize) -> Result<usize> {
            self.asked.push(format!("{message} {options:?}"));
            match self.next(message)? {
                Answer::Pick(text) => Ok(options
                    .iter()
                    .position(|o| o.starts_with(text))
                    .unwrap_or_else(|| panic!("{text:?} not in {options:?}"))),
                other => panic!("expected a pick for {message:?}, got {other:?}"),
            }
        }

        /// Records the first cell of each row; `Pick` must match a first cell exactly.
        fn pick_row(
            &mut self,
            message: &str,
            _header: &[&str],
            rows: &[Vec<String>],
        ) -> Result<usize> {
            let firsts: Vec<&str> = rows.iter().map(|row| row[0].as_str()).collect();
            self.asked.push(format!("{message} {firsts:?}"));
            match self.next(message)? {
                Answer::Pick(text) => Ok(firsts
                    .iter()
                    .position(|first| *first == text)
                    .unwrap_or_else(|| panic!("{text:?} not in {firsts:?}"))),
                other => panic!("expected a pick for {message:?}, got {other:?}"),
            }
        }

        fn text(&mut self, question: TextQuestion<'_>) -> Result<String> {
            self.asked.push(question.message.to_string());
            // Like the terminal: a rejected answer keeps the question open.
            loop {
                let Answer::Text(raw) = self.next(question.message)? else {
                    panic!("expected text for {:?}", question.message);
                };
                let value = match (raw, question.default) {
                    ("", Some(default)) => default.to_string(),
                    _ => raw.to_string(),
                };
                match question.validate.as_ref().map(|v| v(&value)) {
                    Some(Err(message)) => self.rejected.push(message),
                    _ => return Ok(value),
                }
            }
        }

        fn confirm(&mut self, message: &str, _default: bool) -> Result<bool> {
            self.asked.push(message.to_string());
            match self.next(message)? {
                Answer::Yes => Ok(true),
                Answer::No => Ok(false),
                other => panic!("expected yes/no for {message:?}, got {other:?}"),
            }
        }

        fn password(&mut self, message: &str, confirm: bool) -> Result<Zeroizing<String>> {
            self.asked.push(format!(
                "{message} (hidden{})",
                if confirm { ", twice" } else { "" }
            ));
            match self.next(message)? {
                Answer::Secret(secret) => Ok(Zeroizing::new(secret.to_string())),
                other => panic!("expected a secret for {message:?}, got {other:?}"),
            }
        }
    }
}
