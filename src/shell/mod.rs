use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::parser::syntax::CommandNode;

/// Mutable shell state that built-ins read from or write to.
pub struct DpShell {
    /// Command history — newest entry at the back.
    pub history: Vec<String>,
    /// Named aliases: name → expansion string.
    pub aliases: HashMap<String, String>,
    /// Read-only variable names (set by `readonly`).
    pub readonly_vars: std::collections::HashSet<String>,
    /// Directory stack for pushd/popd (top = back of Vec).
    pub dir_stack: Vec<PathBuf>,
    /// Signal trap table: signal name/number → command string.
    pub traps: HashMap<String, String>,
    /// Shell variables (separate from env — set by `set VAR=val`).
    pub shell_vars: HashMap<String, String>,
    /// Shell function table: name → body AST node.
    pub functions: RefCell<HashMap<String, CommandNode>>,
}

impl DpShell {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            aliases: HashMap::new(),
            readonly_vars: std::collections::HashSet::new(),
            dir_stack: Vec::new(),
            traps: HashMap::new(),
            shell_vars: HashMap::new(),
            functions: RefCell::new(HashMap::new()),
        }
    }

    /// Push a raw input line onto the history, skipping blank lines and
    /// consecutive duplicates (matches bash `ignoreboth` behaviour).
    pub fn push_history(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if self.history.last().map(|l| l.as_str()) == Some(line) {
            return;
        }
        self.history.push(line.to_string());
    }
}

impl Default for DpShell {
    fn default() -> Self {
        Self::new()
    }
}
