use crate::lsp::{LspClient, LspInput};
use lsp_types::{Position, Range, Url};
use ropey::Rope;
use std::cell::Cell;
use std::cmp::min;
use std::io::Read;
use std::ops::Deref;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

pub struct Buffer {
    rope: Rope,
    cursor: Cell<usize>,
    version: AtomicI32,
    pub url: Option<Url>,
    pub lsp_client: Option<Arc<LspClient>>,
}

impl Buffer {
    pub fn from_reader<R: Read>(reader: R) -> Self {
        Self {
            rope: Rope::from_reader(reader).unwrap(),
            cursor: Cell::new(0),
            version: Default::default(),
            url: None,
            lsp_client: None,
        }
    }

    pub fn from_reader_lsp<R: Read>(reader: R, url: Url, lsp_client: Arc<LspClient>) -> Self {
        Self {
            rope: Rope::from_reader(reader).unwrap(),
            cursor: Cell::new(0),
            version: Default::default(),
            url: Some(url),
            lsp_client: Some(lsp_client),
        }
    }
}

pub enum Movement {
    Up,
    Down,
    Left,
    Right,
}

pub enum Action {
    Insert(String),
    Backspace,
    Delete,
}

impl Buffer {
    pub fn line_bounds(&self, line: usize) -> (usize, usize) {
        let start = if line > self.rope.len_lines() {
            self.rope.len_chars()
        } else {
            self.rope.line_to_char(line)
        };
        let mut end = if line + 1 > self.rope.len_lines() {
            self.rope.len_chars()
        } else {
            self.rope.line_to_char(line + 1)
        };

        if end != self.rope.len_chars() {
            end -= 1
        }

        loop {
            if start == end {
                break;
            }
            let last = self.rope().chars_at(end - 1).next();
            if let Some('\n' | '\r') = last {
                end -= 1;
            } else {
                break;
            }
        }

        (start, end)
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn col(&self) -> usize {
        self.col_at(self.cursor())
    }

    pub fn row(&self) -> usize {
        self.row_at(self.cursor())
    }

    pub fn row_at(&self, cur: usize) -> usize {
        self.rope.char_to_line(cur)
    }

    pub fn col_at(&self, cur: usize) -> usize {
        let bounds = self.line_bounds(self.row_at(cur));
        cur - bounds.0
    }

    pub fn move_cursor(&self, m: Movement) -> bool {
        let cur = self.cursor.get();
        let line = self.row();

        let prev_line = self.line_bounds(line.saturating_sub(1));
        let curr_line = self.line_bounds(line);
        let next_line = self.line_bounds(line.saturating_add(1));

        let max = self.rope.len_chars();
        let new = match m {
            Movement::Up => prev_line.0 + min(prev_line.1 - prev_line.0, cur - curr_line.0),
            Movement::Down => {
                if line >= self.rope.len_lines() - 1 {
                    cur
                } else {
                    next_line.0 + min(next_line.1 - next_line.0, cur - curr_line.0)
                }
            }
            Movement::Left => {
                let next = cur.saturating_sub(1);
                if next < curr_line.0 {
                    prev_line.1
                } else {
                    next
                }
            }
            Movement::Right => {
                let next = cur.saturating_add(1);
                if next > curr_line.1 {
                    next_line.0
                } else {
                    next
                }
            }
        };

        self.cursor.set(min(new, max));

        if let Some(lsp_client) = &self.lsp_client {
            if let Some(url) = &self.url {
                lsp_client.deref().input_channel.send(LspInput::Cursor {
                    url: url.clone(),
                    row: self.row() as u32,
                    col: self.col() as u32,
                });
            }
        }

        false
    }

    pub fn remove_chars(&mut self, mut start: usize, mut end: usize) {
        if start > self.rope.len_chars() {
            start = self.rope.len_chars()
        }
        if end > self.rope.len_chars() {
            end = self.rope.len_chars()
        }

        if start == end {
            return;
        }

        // delete crlf in one block
        let start_line = self.rope.char_to_line(start);
        let start_bounds = self.line_bounds(start_line);
        if start > start_bounds.1 {
            start = start_bounds.1;
        }

        let end_line = self.rope.char_to_line(end);
        let end_bounds = self.line_bounds(end_line);
        if end > end_bounds.1 {
            end = self.line_bounds(end_line.saturating_add(1)).0;
        }

        let curr = self.cursor();
        if curr >= end {
            self.cursor.set(curr - (end - start))
        } else if curr >= start {
            self.cursor.set(start)
        }

        let start_pos = self.lsp_pos(start);
        let end_pos = self.lsp_pos(end);

        self.rope.remove(start..end);

        if let Some(lsp_client) = &self.lsp_client {
            if let Some(url) = &self.url {
                lsp_client.deref().input_channel.send(LspInput::Edit {
                    url: url.clone(),
                    version: self.version.fetch_add(1, Ordering::SeqCst),
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    text: "".into(),
                });
            }
        }
    }

    pub fn lsp_pos(&self, cur: usize) -> Position {
        Position {
            line: self.row_at(cur) as u32,
            character: self.col_at(cur) as u32,
        }
    }

    pub fn insert(&mut self, start: usize, chars: &str) {
        let curr = self.cursor();
        if curr >= start {
            self.cursor.set(curr + chars.chars().count())
        }
        self.rope.insert(start, chars);

        let start_pos = self.lsp_pos(start);
        let end_pos = self.lsp_pos(start + chars.chars().count());

        if let Some(lsp_client) = &self.lsp_client {
            if let Some(url) = &self.url {
                lsp_client.deref().input_channel.send(LspInput::Edit {
                    url: url.clone(),
                    version: self.version.fetch_add(1, Ordering::SeqCst),
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    text: chars.into(),
                });
            }
        }
    }

    pub fn do_action(&mut self, a: Action) -> bool {
        let curr = self.cursor();
        match a {
            Action::Insert(chars) => {
                self.insert(curr, chars.as_str());
            }
            Action::Backspace => {
                self.remove_chars(curr.saturating_sub(1), curr);
            }
            Action::Delete => {
                self.remove_chars(curr, curr.saturating_add(1));
            }
        }
        true
    }

    pub fn cursor(&self) -> usize {
        self.cursor.get()
    }

    pub fn text(&self) -> &str {
        self.rope.slice(..).as_str().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::buffer::{Buffer, Movement};
    use std::io::Cursor;

    #[test]
    fn edit() {
        let mut buf = Buffer::from_reader(Cursor::new("test"));
        buf.insert(1, "yay");
        assert_eq!(buf.text(), "tyayest");
        buf.remove_chars(1, 5);
        assert_eq!(buf.text(), "tst");
        buf.insert(3, "\nnew line");
        assert_eq!(2, buf.rope().len_lines())
    }

    #[test]
    fn bounds_3() {
        let input = "{\na}";
        let buf = Buffer::from_reader(Cursor::new(input));
        assert_eq!(buf.line_bounds(0), (0, 1));
        assert_eq!(buf.line_bounds(1), (2, 4));
    }

    #[test]
    fn bounds() {
        let str = r#"
a
vv
cc

c
        "#
        .trim()
        .to_string();

        let buf = Buffer::from_reader(Cursor::new(str));
        let b = buf.line_bounds(0);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "a");
        let b = buf.line_bounds(1);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "vv");
        let b = buf.line_bounds(2);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "cc");
        let b = buf.line_bounds(3);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "");
        let b = buf.line_bounds(4);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "c");
    }

    #[test]
    fn bounds_2() {
        let str = r#"
{
  "hello": "hey",
  "name": "salut"
}
        "#
        .trim()
        .to_string();

        let buf = Buffer::from_reader(Cursor::new(str));
        let b = buf.line_bounds(0);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "{");
        let b = buf.line_bounds(1);
        assert_eq!(
            buf.rope.slice(b.0..b.1).as_str().unwrap(),
            "  \"hello\": \"hey\","
        );
        let b = buf.line_bounds(2);
        assert_eq!(
            buf.rope.slice(b.0..b.1).as_str().unwrap(),
            "  \"name\": \"salut\""
        );
        let b = buf.line_bounds(3);
        assert_eq!(buf.rope.slice(b.0..b.1).as_str().unwrap(), "}");
    }

    #[test]
    fn movement() {
        let str = r#"
test
abc
xyzefv
        "#
        .trim()
        .to_string();

        let b = Buffer::from_reader(Cursor::new(str));
        assert_eq!(b.cursor(), 0);
        b.move_cursor(Movement::Right);
        assert_eq!(b.cursor(), 1);
        b.move_cursor(Movement::Right);
        assert_eq!(b.cursor(), 2);
        b.move_cursor(Movement::Right);
        assert_eq!(b.cursor(), 3);
        b.move_cursor(Movement::Right);
        assert_eq!(b.cursor(), 4);
        b.move_cursor(Movement::Down);
        assert_eq!(b.cursor(), 8);
        b.move_cursor(Movement::Left);
        b.move_cursor(Movement::Up);
        assert_eq!(b.cursor(), 2);
        b.move_cursor(Movement::Up);
        assert_eq!(b.cursor(), 2);
        b.move_cursor(Movement::Left);
        b.move_cursor(Movement::Left);
        b.move_cursor(Movement::Left);
        b.move_cursor(Movement::Left);
        b.move_cursor(Movement::Left);
        assert_eq!(b.cursor(), 0);
        b.move_cursor(Movement::Right);
        assert_eq!(b.cursor(), 1);
        b.move_cursor(Movement::Down);
        assert_eq!(b.cursor(), 6);
        b.move_cursor(Movement::Down);
        assert_eq!(b.cursor(), 10);
        b.move_cursor(Movement::Down);
        assert_eq!(b.cursor(), 10);
        b.move_cursor(Movement::Right);
        b.move_cursor(Movement::Right);
        b.move_cursor(Movement::Right);
        b.move_cursor(Movement::Right);
        b.move_cursor(Movement::Right);
        b.move_cursor(Movement::Right);
        b.move_cursor(Movement::Right);
        assert_eq!(b.cursor(), 15);
    }
}
