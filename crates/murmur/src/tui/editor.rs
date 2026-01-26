#[derive(Debug, Clone)]
pub struct Editor {
    pub buffer: String,
    pub history: Vec<String>,
    pub history_cursor: Option<usize>,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            history: Vec::new(),
            history_cursor: None,
        }
    }

    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.history_cursor = None;
    }

    pub fn insert_char(&mut self, ch: char) {
        self.buffer.push(ch);
        self.history_cursor = None;
    }

    pub fn backspace(&mut self) {
        self.buffer.pop();
        self.history_cursor = None;
    }

    pub fn insert_newline(&mut self) {
        self.buffer.push('\n');
        self.history_cursor = None;
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let next = match self.history_cursor {
            None => self.history.len().saturating_sub(1),
            Some(idx) => idx.saturating_sub(1),
        };
        self.history_cursor = Some(next);
        self.buffer = self.history[next].clone();
    }

    pub fn history_next(&mut self) {
        let Some(idx) = self.history_cursor else {
            return;
        };

        if idx + 1 >= self.history.len() {
            self.history_cursor = None;
            self.buffer.clear();
            return;
        }

        let next = idx + 1;
        self.history_cursor = Some(next);
        self.buffer = self.history[next].clone();
    }

    pub fn take_submit(&mut self) -> Option<String> {
        if self.buffer.trim().is_empty() {
            return None;
        }

        let msg = std::mem::take(&mut self.buffer);
        self.history_cursor = None;
        if self.history.last().map(|h| h.as_str()) != Some(msg.as_str()) {
            self.history.push(msg.clone());
        }
        Some(msg)
    }

    pub fn visual_lines(&self) -> usize {
        visual_lines(&self.buffer)
    }

    pub fn visual_lines_wrapped(&self, width: usize) -> usize {
        visual_lines_wrapped(&self.buffer, width)
    }
}

pub fn visual_lines(buffer: &str) -> usize {
    buffer.split('\n').count().max(1)
}

pub fn visual_lines_wrapped(buffer: &str, width: usize) -> usize {
    if width == 0 {
        return visual_lines(buffer);
    }

    let mut total: usize = 0;
    let mut iter = buffer.split('\n').peekable();
    while let Some(line) = iter.next() {
        let is_last = iter.peek().is_none();
        let mut display_width = unicode_width::UnicodeWidthStr::width(line);
        if is_last {
            display_width = display_width.saturating_add(1);
        }
        total = total.saturating_add(rows_for_width(display_width, width));
    }

    total.max(1)
}

fn rows_for_width(display_width: usize, width: usize) -> usize {
    if width == 0 || display_width == 0 {
        return 1;
    }

    (display_width.saturating_sub(1) / width).saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_inserts_and_deletes() {
        let mut e = Editor::new();
        e.insert_char('h');
        e.insert_char('i');
        assert_eq!(e.buffer, "hi");
        e.backspace();
        assert_eq!(e.buffer, "h");
    }

    #[test]
    fn editor_newline_increases_visual_lines() {
        let mut e = Editor::new();
        assert_eq!(e.visual_lines(), 1);
        e.insert_char('a');
        assert_eq!(e.visual_lines(), 1);
        e.insert_newline();
        assert_eq!(e.visual_lines(), 2);
    }

    #[test]
    fn visual_lines_wrapped_counts_wrapped_rows() {
        assert_eq!(visual_lines_wrapped("abcd", 4), 2); // includes cursor
        assert_eq!(visual_lines_wrapped("abcd", 5), 1);
        assert_eq!(visual_lines_wrapped("ab\ncd", 2), 3);
        assert_eq!(visual_lines_wrapped("", 10), 1);
    }

    #[test]
    fn editor_history_navigation_cycles_expected() {
        let mut e = Editor::new();
        e.buffer = "one".to_owned();
        assert_eq!(e.take_submit(), Some("one".to_owned()));
        e.buffer = "two".to_owned();
        assert_eq!(e.take_submit(), Some("two".to_owned()));

        e.history_prev();
        assert_eq!(e.buffer, "two");
        e.history_prev();
        assert_eq!(e.buffer, "one");

        e.history_next();
        assert_eq!(e.buffer, "two");
        e.history_next();
        assert_eq!(e.buffer, "");
        assert_eq!(e.history_cursor, None);
    }
}
