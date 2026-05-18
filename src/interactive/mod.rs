pub mod bash_completer;
pub mod highlighter;
pub mod hinter;
pub mod smart_completer;

pub use highlighter::DpHighlighter;
pub use hinter::DpHinter;

// interactive/mod.rs — Fish-style interactive features for dpshell.
//
// Three layers:
//   1. Syntax highlighting  (DpHighlighter)
//   2. Autosuggestions      (DpHinter)
//   3. Tab completion       (SmartCompleter or BashModeCompleter)
//
// Completion mode (controlled by config [features] enhance_completion):
//   true  → SmartCompleter: fuzzy matching, ColumnarMenu (10 cols, blue selection).
//   false → BashModeCompleter: classic bash-style completion.  A ColumnarMenu
//           is still registered so that reedline's completion engine can
//           invoke the completer and apply quick_completions / partial_completions.
//           Single match → inline replacement; multiple matches with a common
//           prefix → LCP fill; multiple matches at LCP → menu display.

use reedline::{
    ColumnarMenu, EditCommand, FileBackedHistory, Highlighter, Hinter, KeyCode, KeyModifiers,
    MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu,
};
use std::path::Path;

// ──────────────────────────────────────────────────────────────────────────────
// Colour palette
// ──────────────────────────────────────────────────────────────────────────────

mod style {
    use nu_ansi_term::{Color, Style};

    pub fn command() -> Style {
        Color::Green.bold()
    }
    pub fn builtin() -> Style {
        Color::Cyan.bold()
    }
    pub fn argument() -> Style {
        Style::new()
    }
    pub fn flag() -> Style {
        Color::Blue.normal()
    }
    pub fn string() -> Style {
        Color::Yellow.normal()
    }
    pub fn operator() -> Style {
        Color::Magenta.bold()
    }
    pub fn error() -> Style {
        Color::Red.bold()
    }
    pub fn comment() -> Style {
        Color::DarkGray.italic()
    }
    pub fn suggestion() -> Style {
        Color::DarkGray.normal()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in names (shared with completer modules)
// ──────────────────────────────────────────────────────────────────────────────

pub const BUILTIN_NAMES: &[&str] = &[
    ":", "alias", "bg", "bind", "break", "builtin", "cd", "command", "continue", "dirs", "eval",
    "exec", "exit", "export", "fg", "help", "history", "jobs", "kill", "local", "popd", "pushd",
    "readonly", "return", "set", "shift", "source", "test", "trap", "type", "umask", "unalias",
    "unset", "wait", ".", "[",
];

// ──────────────────────────────────────────────────────────────────────────────
// Feature flags
// ──────────────────────────────────────────────────────────────────────────────

pub struct FeatureFlags {
    pub syntax_highlighting: bool,
    pub auto_suggest: bool,
    pub enhance_completion: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            syntax_highlighting: true,
            auto_suggest: true,
            enhance_completion: true,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Reedline builder
// ──────────────────────────────────────────────────────────────────────────────

pub fn build_editor(history_path: &Path, flags: &FeatureFlags) -> anyhow::Result<Reedline> {
    let history = Box::new(
        FileBackedHistory::with_file(10_000, history_path.to_path_buf())
            .map_err(|e| anyhow::anyhow!("history: {}", e))?,
    );

    let highlighter: Option<Box<dyn Highlighter>> = if flags.syntax_highlighting {
        Some(Box::new(DpHighlighter))
    } else {
        None
    };

    let hinter: Option<Box<dyn Hinter>> = if flags.auto_suggest {
        Some(Box::new(DpHinter::new()))
    } else {
        None
    };

    if flags.enhance_completion {
        // Enhanced completion: fuzzy matching + columnar menu.
        let mut keybindings = reedline::default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu("completion_menu".to_string()),
                ReedlineEvent::MenuNext,
                ReedlineEvent::Edit(vec![EditCommand::Complete]),
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::BackTab,
            ReedlineEvent::MenuPrevious,
        );
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Right,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::MenuRight,
                ReedlineEvent::HistoryHintComplete,
                ReedlineEvent::Edit(vec![EditCommand::MoveRight { select: false }]),
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Char('f'),
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::MenuRight,
                ReedlineEvent::HistoryHintComplete,
                ReedlineEvent::Edit(vec![EditCommand::MoveRight { select: false }]),
            ]),
        );

        let edit_mode = Box::new(reedline::Emacs::new(keybindings));
        let mut rl = Reedline::create()
            .with_history(history)
            .with_edit_mode(edit_mode)
            .with_ansi_colors(true)
            .with_quick_completions(true)
            .with_partial_completions(true);

        if let Some(h) = highlighter {
            rl = rl.with_highlighter(h);
        }
        if let Some(h) = hinter {
            rl = rl.with_hinter(h);
        }

        let completer = Box::new(smart_completer::SmartCompleter::new());
        let menu = Box::new(
            ColumnarMenu::default()
                .with_name("completion_menu")
                .with_marker("")
                .with_columns(10)
                .with_selected_text_style(nu_ansi_term::Color::Blue.bold().reverse())
                .with_selected_match_text_style(nu_ansi_term::Color::Blue.bold().reverse()),
        );
        rl = rl
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(menu));
        Ok(rl)
    } else {
        // Bash-style completion.
        //
        // A ColumnarMenu MUST be registered even in bash mode because
        // reedline's completion engine only invokes the completer inside
        // the menu-handling code paths (ReedlineEvent::Menu activates the
        // menu and calls menu.update_values(), which calls the completer;
        // EditCommand::Complete alone is a no-op unless an active menu
        // already exists).  Without a registered menu, Tab does nothing.
        //
        // The BashModeCompleter returns all raw suggestions and lets
        // reedline handle the rest:
        //   - quick_completions  → single match: inline replacement
        //   - partial_completions → multiple matches with LCP: fill to LCP
        //   - ColumnarMenu       → multiple matches at LCP: display candidates
        let mut keybindings = reedline::default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu("completion_menu".to_string()),
                ReedlineEvent::MenuNext,
                ReedlineEvent::HistoryHintComplete,
                ReedlineEvent::Edit(vec![EditCommand::Complete]),
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::BackTab,
            ReedlineEvent::MenuPrevious,
        );
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Right,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::MenuRight,
                ReedlineEvent::HistoryHintComplete,
                ReedlineEvent::Edit(vec![EditCommand::MoveRight { select: false }]),
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Char('f'),
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::MenuRight,
                ReedlineEvent::HistoryHintComplete,
                ReedlineEvent::Edit(vec![EditCommand::MoveRight { select: false }]),
            ]),
        );

        let edit_mode = Box::new(reedline::Emacs::new(keybindings));
        let mut rl = Reedline::create()
            .with_history(history)
            .with_edit_mode(edit_mode)
            .with_ansi_colors(true)
            .with_quick_completions(true)
            .with_partial_completions(true);

        if let Some(h) = highlighter {
            rl = rl.with_highlighter(h);
        }
        if let Some(h) = hinter {
            rl = rl.with_hinter(h);
        }

        let completer = Box::new(bash_completer::BashModeCompleter::new());
        let menu = Box::new(
            ColumnarMenu::default()
                .with_name("completion_menu")
                .with_columns(10),
        );
        rl = rl
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(menu));
        Ok(rl)
    }
}
