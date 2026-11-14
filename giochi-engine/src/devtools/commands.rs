//! The command table behind the in-game console.
//!
//! A command is a name, a one-line help string and either a **built-in** the engine
//! executes (it needs engine state: FPS, time scale, entity count) or a **handler** the
//! game registered. Handlers close over whatever the game needs, which in a
//! single-threaded engine means `Rc<RefCell<_>>` — the demo shows the pattern.
//!
//! Registration is by name and replaces: registering `spawn` a second time is how a game
//! overrides the built-in with its own version, and that is deliberate, not an accident.

/// The built-in commands, as a value the engine can match on.
///
/// An enum rather than a function pointer because the built-ins need `&mut` access to
/// the debug state, and because it keeps the console's `help` output in a fixed order.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// Runs no built-in; the entry is a game handler.
    None = 0,
    /// Lists every command with its help text.
    Help = 1,
    /// Clears the console's scrollback.
    Clear = 2,
    /// Prints FPS, frame time and the p99 window.
    Fps = 3,
    /// Prints the entity count the game reported.
    Entities = 4,
    /// `time_scale <factor>`: sets the fixed-step multiplier.
    TimeScale = 5,
    /// Toggles the god-mode flag. The game reads it; the engine does not.
    God = 6,
    /// Toggles the noclip flag.
    Noclip = 7,
    /// `spawn <kind> [count]`: queues a request the game drains.
    Spawn = 8,
    /// Asks the renderer to recompile everything it can.
    ReloadShaders = 9,
    /// Prints the profiler's window summary.
    DumpFrameStats = 10,
}

impl Builtin {
    /// Whether anything reads the `god`/`noclip` flags.
    ///
    /// The engine does not, and that is the point: `god` is a *game* concept the devtools
    /// console offers a switch for. A game that never reads it sees the toggle do nothing,
    /// which is better than the engine inventing a behaviour it cannot enforce.
    pub const fn is_game_flag(self) -> bool {
        matches!(self, Self::God | Self::Noclip)
    }
}

/// A game's command. Receives the argument string (everything after the name, trimmed)
/// and returns what to print.
///
/// Returning an empty string prints nothing, which is what a command that only changes
/// state wants.
pub type CommandFn = Box<dyn FnMut(&str) -> String>;

/// One entry in the table.
struct Entry {
    name: String,
    help: String,
    builtin: Builtin,
    handler: Option<CommandFn>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.name)
            .field("help", &self.help)
            .field("builtin", &self.builtin)
            .field("has_handler", &self.handler.is_some())
            .finish()
    }
}

/// The console's command table.
///
/// ```no_run
/// use giochi_engine::devtools::CommandRegistry;
///
/// let mut commands = CommandRegistry::with_builtins();
/// commands.register("heal", "restores the player's health", Box::new(|args| {
///     format!("healed by {}", args.trim())
/// }));
/// assert!(commands.contains("heal"));
/// ```
///
/// # C ABI
///
/// Not exposed. Phase 24 exposes `giochi_console_execute`, which runs the same table.
#[repr(C)]
pub struct CommandRegistry {
    /// Sorted by name so `help` and autocomplete are stable across runs.
    entries: Vec<Entry>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("commands", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    /// An empty table. Use [`Self::with_builtins`] for the one the engine installs.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            _reserved: [0; 4],
        }
    }

    /// The table the engine installs: `help`, `clear`, `fps`, `entities`, `time_scale`,
    /// `god`, `noclip`, `spawn`, `reload_shaders`, `dump_frame_stats`.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register_builtin("help", "lists the commands and what they do", Builtin::Help);
        registry.register_builtin("clear", "empties the console scrollback", Builtin::Clear);
        registry.register_builtin("fps", "shows FPS, frame time and p99", Builtin::Fps);
        registry.register_builtin(
            "entities",
            "shows how many entities the game reported",
            Builtin::Entities,
        );
        registry.register_builtin(
            "time_scale",
            "sets the simulation speed: time_scale <factor>",
            Builtin::TimeScale,
        );
        registry.register_builtin("god", "toggles the game's god-mode flag", Builtin::God);
        registry.register_builtin("noclip", "toggles the game's noclip flag", Builtin::Noclip);
        registry.register_builtin(
            "spawn",
            "queues a spawn the game handles: spawn <kind> [count]",
            Builtin::Spawn,
        );
        registry.register_builtin(
            "reload_shaders",
            "recompiles every shader that can be recompiled",
            Builtin::ReloadShaders,
        );
        registry.register_builtin(
            "dump_frame_stats",
            "prints the profiler window summary",
            Builtin::DumpFrameStats,
        );
        registry
    }

    /// Registers a game command, replacing one with the same name.
    ///
    /// Returns `false` for a blank name, which would be unreachable from the console.
    pub fn register(&mut self, name: &str, help: &str, handler: CommandFn) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.replace(Entry {
            name: name.to_owned(),
            help: help.to_owned(),
            builtin: Builtin::None,
            handler: Some(handler),
        });
        true
    }

    /// Registers (or replaces) a built-in.
    pub fn register_builtin(&mut self, name: &str, help: &str, builtin: Builtin) -> bool {
        let name = name.trim();
        if name.is_empty() || builtin == Builtin::None {
            return false;
        }
        self.replace(Entry {
            name: name.to_owned(),
            help: help.to_owned(),
            builtin,
            handler: None,
        });
        true
    }

    /// Inserts or replaces, keeping the table sorted.
    fn replace(&mut self, entry: Entry) {
        match self
            .entries
            .binary_search_by(|known| known.name.cmp(&entry.name))
        {
            Ok(index) => self.entries[index] = entry,
            Err(index) => self.entries.insert(index, entry),
        }
    }

    /// Every command name, sorted.
    pub fn names(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// How many commands are registered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The help text for `name`, or `None` when it is not registered.
    pub fn help_for(&self, name: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.help.clone())
    }

    /// The built-in behind `name`, or [`Builtin::None`] for a handler or an unknown name.
    pub fn builtin_for(&self, name: &str) -> Builtin {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map_or(Builtin::None, |entry| entry.builtin)
    }

    /// Whether `name` is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    /// Runs the handler registered for `name` with `args`.
    ///
    /// Returns `None` when the name is unknown **or** when it is a built-in: built-ins
    /// need engine state the table does not have, so they are executed by the devtools
    /// facade instead. The distinction is what keeps this type testable without a
    /// renderer.
    pub fn run_handler(&mut self, name: &str, args: &str) -> Option<String> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.name == name && entry.handler.is_some())?;
        entry.handler.as_mut().map(|handler| handler(args))
    }

    /// The command names starting with `prefix`, sorted.
    ///
    /// Used by Tab completion and by the hint line under the input box.
    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let prefix = prefix.trim();
        self.entries
            .iter()
            .filter(|entry| entry.name.starts_with(prefix))
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// `name` and its help, one line per command, for the `help` command.
    pub fn help_lines(&self) -> Vec<String> {
        let width = self
            .entries
            .iter()
            .map(|entry| entry.name.len())
            .max()
            .unwrap_or(0);
        self.entries
            .iter()
            .map(|entry| format!("{:<width$}  {}", entry.name, entry.help))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_table_has_no_commands() {
        let registry = CommandRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.names().is_empty());
    }

    #[test]
    fn the_builtins_are_the_ten_the_phase_asks_for() {
        let registry = CommandRegistry::with_builtins();
        assert_eq!(registry.len(), 10);
        for name in [
            "help",
            "clear",
            "fps",
            "entities",
            "time_scale",
            "god",
            "noclip",
            "spawn",
            "reload_shaders",
            "dump_frame_stats",
        ] {
            assert!(registry.contains(name), "{name} must exist");
            assert!(
                registry.help_for(name).is_some_and(|help| !help.is_empty()),
                "{name} must document itself"
            );
        }
    }

    #[test]
    fn names_and_help_are_sorted_so_the_output_is_stable() {
        let registry = CommandRegistry::with_builtins();
        let names = registry.names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn a_game_command_can_be_registered_and_run() {
        let mut registry = CommandRegistry::new();
        assert!(registry.register(
            "heal",
            "restores health",
            Box::new(|args| { format!("healed {args}") })
        ));
        assert_eq!(
            registry.run_handler("heal", "3"),
            Some("healed 3".to_owned())
        );
    }

    #[test]
    fn a_blank_name_is_refused() {
        let mut registry = CommandRegistry::new();
        assert!(!registry.register("   ", "nothing", Box::new(|_| String::new())));
        assert!(!registry.register_builtin("", "nothing", Builtin::Help));
        assert!(registry.is_empty());
    }

    #[test]
    fn registering_the_same_name_replaces_it() {
        let mut registry = CommandRegistry::new();
        registry.register("x", "first", Box::new(|_| "one".to_owned()));
        registry.register("x", "second", Box::new(|_| "two".to_owned()));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.help_for("x").as_deref(), Some("second"));
        assert_eq!(registry.run_handler("x", ""), Some("two".to_owned()));
    }

    #[test]
    fn a_game_command_can_override_a_builtin() {
        let mut registry = CommandRegistry::with_builtins();
        registry.register(
            "spawn",
            "the game's spawn",
            Box::new(|args| format!("game spawned {args}")),
        );
        assert_eq!(registry.builtin_for("spawn"), Builtin::None);
        assert_eq!(
            registry.run_handler("spawn", "cube 5"),
            Some("game spawned cube 5".to_owned())
        );
    }

    #[test]
    fn a_builtin_has_no_handler_so_the_facade_runs_it() {
        let mut registry = CommandRegistry::with_builtins();
        assert_eq!(registry.builtin_for("fps"), Builtin::Fps);
        assert_eq!(registry.run_handler("fps", ""), None);
        assert_eq!(registry.builtin_for("nope"), Builtin::None);
        assert_eq!(registry.run_handler("nope", ""), None);
    }

    #[test]
    fn completion_matches_a_prefix_and_nothing_else() {
        let registry = CommandRegistry::with_builtins();
        assert_eq!(registry.complete("time"), vec!["time_scale".to_owned()]);
        assert_eq!(
            registry.complete("re"),
            vec!["reload_shaders".to_owned()],
            "only registered names, and sorted"
        );
        assert!(registry.complete("zzz").is_empty());
        assert_eq!(registry.complete("").len(), registry.len());
    }

    #[test]
    fn help_lines_align_the_descriptions() {
        let registry = CommandRegistry::with_builtins();
        let lines = registry.help_lines();
        assert_eq!(lines.len(), registry.len());
        assert!(lines.iter().all(|line| line.contains("  ")));
    }

    #[test]
    fn only_god_and_noclip_are_game_flags() {
        assert!(Builtin::God.is_game_flag());
        assert!(Builtin::Noclip.is_game_flag());
        assert!(!Builtin::Fps.is_game_flag());
        assert!(!Builtin::None.is_game_flag());
    }
}
