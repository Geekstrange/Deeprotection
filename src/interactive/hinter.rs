use reedline::{DefaultHinter, Hinter};

use super::style;

pub struct DpHinter {
    inner: DefaultHinter,
}

impl DpHinter {
    pub fn new() -> Self {
        Self {
            inner: DefaultHinter::default().with_style(style::suggestion()),
        }
    }
}

impl Hinter for DpHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        history: &dyn reedline::History,
        use_ansi_coloring: bool,
        cwd: &str,
    ) -> String {
        self.inner
            .handle(line, pos, history, use_ansi_coloring, cwd)
    }

    fn complete_hint(&self) -> String {
        self.inner.complete_hint()
    }
    fn next_hint_token(&self) -> String {
        self.inner.next_hint_token()
    }
}
