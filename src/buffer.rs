use std::cmp::min;
use std::io::Read;
use std::sync::atomic::{AtomicI32, Ordering};

use lsp_types::{Position, Range, Url};
use ropey::Rope;

use crate::lsp::{LspCompletion, LspInput, LspLang};

pub enum BufferSource {
    Text,
    File { uri: Url },
}

pub struct Buffer {
    pub(crate) source: BufferSource,
    rope: Rope,
    cursor: usize,
    version: AtomicI32,
    pub completions: Vec<LspCompletion>,
}

impl Buffer {
    pub fn position_to_index(&self, pos: Position) -> usize {
        let line = self.line_bounds(pos.line as usize);
        line.0 + pos.character as usize
    }

    pub fn range_to_bounds(&self, range: &Range) -> (usize, usize) {
        (
            self.position_to_index(range.start),
            self.position_to_index(range.end),
        )
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
    pub fn from_reader<R: Read>(reader: R, src: BufferSource) -> Self {
        Self {
            rope: Rope::from_reader(reader).unwrap(),
            cursor: 0,
            version: Default::default(),
            completions: vec![],
            source: src,
        }
    }

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

    pub fn move_cursor(&mut self, m: Movement) -> bool {
        let cur = self.cursor;
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

        self.cursor = min(new, max);

        self.completions = vec![];

        false
    }

    pub fn remove_chars(&mut self, mut start: usize, mut end: usize) -> Option<LspInput> {
        if start > self.rope.len_chars() {
            start = self.rope.len_chars()
        }
        if end > self.rope.len_chars() {
            end = self.rope.len_chars()
        }

        if start == end {
            return None;
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
            self.cursor = curr - (end - start)
        } else if curr >= start {
            self.cursor = start
        }

        // let start_pos = self.lsp_pos(start);
        // let end_pos = self.lsp_pos(end);

        self.rope.remove(start..end);

        self.lsp_edit()
    }

    pub fn lsp_pos(&self, cur: usize) -> Position {
        Position {
            line: self.row_at(cur) as u32,
            character: self.col_at(cur) as u32,
        }
    }

    pub fn insert(&mut self, start: usize, chars: &str) -> Option<LspInput> {
        let curr = self.cursor();
        if curr >= start {
            self.cursor = curr + chars.chars().count()
        }
        self.rope.insert(start, chars);

        // let start_pos = self.lsp_pos(start);
        // let end_pos = self.lsp_pos(start + chars.chars().count());

        self.lsp_edit()
    }

    fn lsp_edit(&mut self) -> Option<LspInput> {
        if let BufferSource::File { uri } = &self.source {
            Some(LspInput::Edit {
                uri: uri.clone(),
                version: self.version.fetch_add(1, Ordering::SeqCst),
                text: self.text().to_string(),
            })
        } else {
            None
        }
    }

    pub fn do_action(&mut self, a: Action) -> Option<LspInput> {
        let curr = self.cursor();
        match a {
            Action::Insert(chars) => self.insert(curr, chars.as_str()),
            Action::Backspace => self.remove_chars(curr.saturating_sub(1), curr),
            Action::Delete => self.remove_chars(curr, curr.saturating_add(1)),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn text(&self) -> &str {
        self.rope.slice(..).as_str().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::buffer::{Buffer, BufferSource, Movement};

    #[test]
    fn edit() {
        let mut buf = Buffer::from_reader(Cursor::new("test"), BufferSource::Text);
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
        let buf = Buffer::from_reader(Cursor::new(input), BufferSource::Text);
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

        let buf = Buffer::from_reader(Cursor::new(str), BufferSource::Text);
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

        let buf = Buffer::from_reader(Cursor::new(str), BufferSource::Text);
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

        let mut b = Buffer::from_reader(Cursor::new(str), BufferSource::Text);
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
