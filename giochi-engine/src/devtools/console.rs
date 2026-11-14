//! The in-game console: scrollback, input line, history and completion.
//!
//! The state machine is separate from the drawing so it can be tested without a window,
//! and the drawing is separate from the key handling so the engine can decide which keys
//! the console owns before egui ever sees them. That split matters: `Enter` submits a
//! command here, not inside a text widget, because the console is a command line and not
//! a form.
//!
//! # Keys the console takes
//!
//! | Key | Effect |
//! |---|---|
//! | `Enter` | submit the current line (`Console::submit`) |
//! | `Tab` | complete to the longest common prefix, or cycle when there is only one |
//! | `Up` / `Down` | walk the submitted history |
//! | `Escape` | close |
//!
//! Everything else goes to egui's text editor: arrows within the line, `Home`/`End`,
//! `Backspace`, `Delete`, and typed characters (which arrive as `TextInput`, not as
//! keys).

use std::collections::VecDeque;

use sdl2::keyboard::Keycode;

/// How many submitted lines the history keeps. The plan's number.
pub const MAX_HISTORY: usize = 100;

/// How many scrollback lines the console keeps.
///
/// Chosen so a `help` dump plus a session's diagnostics stay on screen without the panel
/// growing without bound; the oldest line is dropped, which is what a terminal does.
pub const MAX_OUTPUT: usize = 512;

/// What a scrollback line came from, which decides its colour.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Something the user typed.
    Input = 0,
    /// Normal command output.
    Output = 1,
    /// A command that failed, or an unknown command.
    Error = 2,
    /// A system notice (console opened, hot reload fired, …).
    Notice = 3,
}

/// One line of the scrollback.
#[repr(C)]
#[derive(Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    kind: LineKind,
    text: String,
    _reserved: [u32; 4],
}

impl ConsoleLine {
    /// Creates a line.
    pub fn new(kind: LineKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            _reserved: [0; 4],
        }
    }

    /// What produced it.
    pub fn kind(&self) -> LineKind {
        self.kind
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Debug for ConsoleLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.text)
    }
}

/// What a key press did to the console.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleAction {
    /// The key was not the console's.
    Ignored = 0,
    /// The key was consumed and nothing else happened.
    Consumed = 1,
    /// `Enter`: the current line is ready to run (`Console::take_line`).
    Submit = 2,
    /// `Tab`: completion ran.
    Completed = 3,
    /// `Escape`: the console asked to close.
    Close = 4,
}

/// The console's state.
///
/// # C ABI
///
/// Not exposed. Phase 24 reaches it through `giochi_console_execute` and the panel.
#[repr(C)]
pub struct Console {
    open: bool,
    /// The line being edited.
    input: String,
    output: VecDeque<ConsoleLine>,
    /// Submitted lines, oldest first, capped at [`MAX_HISTORY`].
    history: VecDeque<String>,
    /// Where `Up`/`Down` currently is: `None` means "editing a new line".
    cursor: Option<usize>,
    /// The last completion result, shown under the input until the next edit.
    hint: Option<String>,
    /// The line left over from a `Submit`, for `take_line`.
    pending: Option<String>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for Console {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Console")
            .field("open", &self.open)
            .field("input", &self.input)
            .field("output", &self.output.len())
            .field("history", &self.history.len())
            .finish_non_exhaustive()
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    /// Creates a closed console with the greeting already in the scrollback.
    pub fn new() -> Self {
        let mut console = Self {
            open: false,
            input: String::new(),
            output: VecDeque::new(),
            history: VecDeque::new(),
            cursor: None,
            hint: None,
            pending: None,
            _reserved: [0; 4],
        };
        console.push_line(
            LineKind::Notice,
            "console: type 'help' for the command list, Esc to close",
        );
        console
    }

    /// Whether the console is on screen.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens it. Also resets the history cursor to "new line".
    pub fn open(&mut self) {
        self.open = true;
        self.cursor = None;
        self.hint = None;
    }

    /// Closes it. The scrollback and history survive.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Flips it. Returns the new state.
    pub fn toggle(&mut self) -> bool {
        if self.open {
            self.close();
        } else {
            self.open();
        }
        self.open
    }

    /// The line being edited.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Replaces the line being edited (a paste, or a test).
    pub fn set_input(&mut self, text: impl Into<String>) {
        self.input = text.into();
        self.hint = None;
    }

    /// The line being edited, mutably — what egui's text widget binds to.
    pub(crate) fn input_mut(&mut self) -> &mut String {
        self.hint = None;
        &mut self.input
    }

    /// The hint line under the input, if a completion ran and there were several choices.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// Appends a line to the scrollback, dropping the oldest beyond [`MAX_OUTPUT`].
    pub fn push_line(&mut self, kind: LineKind, text: impl Into<String>) {
        for line in text.into().lines() {
            self.output.push_back(ConsoleLine::new(kind, line));
        }
        while self.output.len() > MAX_OUTPUT {
            self.output.pop_front();
        }
    }

    /// The scrollback, oldest first.
    pub fn output(&self) -> Vec<&ConsoleLine> {
        self.output.iter().collect()
    }

    /// The scrollback as plain text, for a test or a log dump.
    pub fn output_text(&self) -> Vec<String> {
        self.output.iter().map(|line| line.text.clone()).collect()
    }

    /// How many lines the scrollback holds.
    pub fn output_len(&self) -> usize {
        self.output.len()
    }

    /// Empties the scrollback (the `clear` command).
    pub fn clear(&mut self) {
        self.output.clear();
        self.hint = None;
    }

    /// The submitted history, oldest first.
    pub fn history(&self) -> Vec<String> {
        self.history.iter().cloned().collect()
    }

    /// Adds a line to the history without running it.
    ///
    /// Used by `submit`; a game replaying a command file can call it directly.
    pub fn push_history(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        // Repeating the previous command is normal in a shell; storing it twice is not.
        if self.history.back().is_some_and(|last| last == line) {
            return;
        }
        self.history.push_back(line.to_owned());
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
        self.cursor = None;
    }

    /// Sets the input to the submitted line and returns it, or `None` for a blank line.
    ///
    /// Also echoes the line into the scrollback and the history, which is what makes the
    /// panel readable afterwards.
    pub fn submit(&mut self) -> Option<String> {
        let line = self.input.trim().to_owned();
        self.input.clear();
        self.hint = None;
        self.cursor = None;
        if line.is_empty() {
            return None;
        }
        self.push_line(LineKind::Input, format!("> {line}"));
        self.push_history(&line);
        self.pending = Some(line.clone());
        Some(line)
    }

    /// Takes the line a previous [`Self::submit`] produced.
    pub fn take_line(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// Completes the current input against `candidates`.
    ///
    /// One candidate fills the line in and appends a space. Several extend the line to the
    /// longest common prefix and put the list in the hint. None leaves everything alone.
    /// Returns what happened, mostly so a caller can decide to log it.
    pub fn complete(&mut self, candidates: &[String]) -> ConsoleAction {
        if candidates.is_empty() {
            return ConsoleAction::Ignored;
        }
        if candidates.len() == 1 {
            self.input = format!("{} ", candidates[0]);
            self.hint = None;
            return ConsoleAction::Completed;
        }

        let prefix = common_prefix(candidates);
        if prefix.len() > self.input.trim_end().len() {
            self.input = prefix;
        }
        self.hint = Some(candidates.join("  "));
        ConsoleAction::Completed
    }

    /// Handles a key the console owns.
    ///
    /// `candidates` is what Tab should complete against — the caller passes
    /// `registry.complete(current_word)`, because the console has no command table of its
    /// own.
    pub fn handle_key(&mut self, key: Keycode, candidates: &[String]) -> ConsoleAction {
        if !self.open {
            return ConsoleAction::Ignored;
        }

        match key {
            Keycode::Escape => ConsoleAction::Close,
            Keycode::Return | Keycode::KpEnter => {
                if self.input.trim().is_empty() {
                    ConsoleAction::Consumed
                } else {
                    ConsoleAction::Submit
                }
            }
            Keycode::Tab => self.complete(candidates),
            Keycode::Up => {
                self.history_previous();
                ConsoleAction::Consumed
            }
            Keycode::Down => {
                self.history_next();
                ConsoleAction::Consumed
            }
            _ => ConsoleAction::Ignored,
        }
    }

    /// Moves to the previous history entry, filling the input.
    fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.cursor {
            None => self.history.len() - 1,
            Some(0) => 0,
            Some(index) => index - 1,
        };
        self.cursor = Some(next);
        self.input = self.history[next].clone();
        self.hint = None;
    }

    /// Moves to the next history entry, or back to an empty line past the end.
    fn history_next(&mut self) {
        let Some(index) = self.cursor else {
            return;
        };
        if index + 1 >= self.history.len() {
            self.cursor = None;
            self.input.clear();
            return;
        }
        self.cursor = Some(index + 1);
        self.input = self.history[index + 1].clone();
        self.hint = None;
    }

    /// The word Tab should complete: the command name, or the whole line while there is
    /// no space yet.
    pub fn completion_prefix(&self) -> &str {
        match self.input.split_once(' ') {
            Some((name, _)) => name,
            None => self.input.as_str(),
        }
    }
}

/// The longest common prefix of `candidates`, on a character boundary.
fn common_prefix(candidates: &[String]) -> String {
    let Some(first) = candidates.first() else {
        return String::new();
    };
    let mut length = first.chars().count();
    for candidate in &candidates[1..] {
        length = length.min(
            first
                .chars()
                .zip(candidate.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    first.chars().take(length).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console() -> Console {
        Console::new()
    }

    #[test]
    fn a_new_console_is_closed_and_greets_once() {
        let console = console();
        assert!(!console.is_open());
        assert_eq!(console.output_len(), 1);
        assert_eq!(console.output()[0].kind(), LineKind::Notice);
    }

    #[test]
    fn toggling_flips_the_open_state() {
        let mut console = console();
        assert!(console.toggle());
        assert!(console.is_open());
        assert!(!console.toggle());
        assert!(!console.is_open());
    }

    #[test]
    fn submitting_a_line_echoes_it_and_stores_the_history() {
        let mut console = console();
        console.open();
        console.set_input("  help  ");
        assert_eq!(console.submit(), Some("help".to_owned()));
        assert_eq!(console.input(), "", "the line is cleared");
        assert_eq!(console.history(), vec!["help".to_owned()]);
        assert_eq!(console.take_line(), Some("help".to_owned()));
        assert_eq!(console.take_line(), None, "the line is taken once");
        assert!(
            console.output_text().iter().any(|line| line == "> help"),
            "the echoed line must be in the scrollback"
        );
    }

    #[test]
    fn a_blank_line_is_not_submitted_and_not_stored() {
        let mut console = console();
        console.open();
        console.set_input("   ");
        assert_eq!(console.submit(), None);
        assert!(console.history().is_empty());
    }

    #[test]
    fn the_history_is_capped() {
        let mut console = console();
        console.open();
        for index in 0..(MAX_HISTORY + 20) {
            console.set_input(format!("cmd{index}"));
            console.submit();
        }
        assert_eq!(console.history().len(), MAX_HISTORY);
        let expected = format!("cmd{}", MAX_HISTORY + 19);
        assert_eq!(
            console.history().last().map(String::as_str),
            Some(expected.as_str())
        );
        assert!(
            !console.history().contains(&"cmd0".to_owned()),
            "the oldest entries are dropped"
        );
    }

    #[test]
    fn a_repeated_command_is_stored_once() {
        let mut console = console();
        console.open();
        console.set_input("fps");
        console.submit();
        console.set_input("fps");
        console.submit();
        assert_eq!(console.history(), vec!["fps".to_owned()]);
    }

    #[test]
    fn the_scrollback_is_capped() {
        let mut console = console();
        for index in 0..(MAX_OUTPUT + 30) {
            console.push_line(LineKind::Output, format!("line {index}"));
        }
        assert_eq!(console.output_len(), MAX_OUTPUT);
        assert!(
            !console.output_text().contains(&"line 0".to_owned()),
            "the oldest line is dropped"
        );
    }

    #[test]
    fn a_multi_line_push_becomes_several_lines() {
        let mut console = console();
        let before = console.output_len();
        console.push_line(LineKind::Output, "one\ntwo\nthree");
        assert_eq!(console.output_len(), before + 3);
    }

    #[test]
    fn up_and_down_walk_the_history() {
        let mut console = console();
        console.open();
        for command in ["alpha", "beta", "gamma"] {
            console.set_input(command);
            console.submit();
        }
        assert_eq!(console.input(), "");

        assert_eq!(
            console.handle_key(Keycode::Up, &[]),
            ConsoleAction::Consumed
        );
        assert_eq!(console.input(), "gamma");
        console.handle_key(Keycode::Up, &[]);
        assert_eq!(console.input(), "beta");
        console.handle_key(Keycode::Down, &[]);
        assert_eq!(console.input(), "gamma");
        console.handle_key(Keycode::Down, &[]);
        assert_eq!(console.input(), "", "past the end is a new line");
    }

    #[test]
    fn completion_with_one_candidate_fills_the_line() {
        let mut console = console();
        console.open();
        console.set_input("time");
        assert_eq!(
            console.handle_key(Keycode::Tab, &["time_scale".to_owned()]),
            ConsoleAction::Completed
        );
        assert_eq!(console.input(), "time_scale ");
    }

    #[test]
    fn completion_with_several_candidates_extends_to_the_common_prefix() {
        let mut console = console();
        console.open();
        console.set_input("re");
        let candidates = vec!["reload_shaders".to_owned(), "reset".to_owned()];
        console.complete(&candidates);
        assert_eq!(
            console.input(),
            "re",
            "the prefix is already the common part"
        );
        assert_eq!(
            console.hint(),
            Some("reload_shaders  reset"),
            "the choices must be visible"
        );
    }

    #[test]
    fn completion_never_cuts_a_multi_byte_character() {
        let candidates = vec!["café".to_owned(), "cafézinho".to_owned()];
        assert_eq!(common_prefix(&candidates), "café");
    }

    #[test]
    fn the_completion_prefix_is_the_first_word() {
        let mut console = console();
        console.set_input("spawn cube");
        assert_eq!(console.completion_prefix(), "spawn");
        console.set_input("spawn");
        assert_eq!(console.completion_prefix(), "spawn");
    }

    #[test]
    fn escape_closes_and_empty_enter_does_nothing() {
        let mut console = console();
        console.open();
        assert_eq!(
            console.handle_key(Keycode::Escape, &[]),
            ConsoleAction::Close,
            "the caller closes it; the console only reports the intent"
        );
        assert_eq!(
            console.handle_key(Keycode::Return, &[]),
            ConsoleAction::Consumed,
            "an empty line is not submitted"
        );
        console.set_input("fps");
        assert_eq!(
            console.handle_key(Keycode::Return, &[]),
            ConsoleAction::Submit
        );
    }

    #[test]
    fn a_closed_console_owns_no_key() {
        let mut console = console();
        for key in [Keycode::Escape, Keycode::Return, Keycode::Tab, Keycode::Up] {
            assert_eq!(console.handle_key(key, &[]), ConsoleAction::Ignored);
        }
    }

    #[test]
    fn clear_empties_the_scrollback_but_keeps_the_history() {
        let mut console = console();
        console.open();
        console.set_input("fps");
        console.submit();
        console.clear();
        assert_eq!(console.output_len(), 0);
        assert_eq!(console.history(), vec!["fps".to_owned()]);
    }
}
