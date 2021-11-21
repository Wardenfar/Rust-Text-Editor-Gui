use itertools::Itertools;
use std::cmp::{max, min};
use std::io::Read;
use std::ops::RangeBounds;
use std::sync::atomic::{AtomicI32, Ordering};

use lsp_types::{Position, Range, Url};
use ropey::Rope;

use crate::lsp::{CompletionData, LspCompletion, LspInput};

pub enum BufferSource {
    Text,
    File { uri: Url },
}

pub struct Buffer {
    pub(crate) source: BufferSource,
    rope: Rope,
    cursor: Cursor,
    version: AtomicI32,
    pub completions: Vec<LspCompletion>,
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

pub type Index = usize;
pub type Bounds = (Index, Index);

#[derive(Clone, Debug)]
pub struct Cursor {
    pub head: Index,
    pub tail: Index,
}

impl Cursor {
    pub fn min(&self) -> Index {
        min(self.head, self.tail)
    }

    pub fn max(&self) -> Index {
        max(self.head, self.tail)
    }
}

pub trait FromWithBuffer<T> {
    fn from_with_buf(o: T, buffer: &Buffer) -> Self;
}

pub trait IntoWithBuffer<T> {
    fn into_with_buf(self, buffer: &Buffer) -> T;
}

impl<F, I> IntoWithBuffer<F> for I
where
    F: FromWithBuffer<I>,
{
    fn into_with_buf(self, buffer: &Buffer) -> F {
        F::from_with_buf(self, buffer)
    }
}

impl FromWithBuffer<&Range> for Bounds {
    fn from_with_buf(range: &Range, buffer: &Buffer) -> Self {
        (
            Index::from_with_buf(&range.start, buffer),
            Index::from_with_buf(&range.end, buffer),
        )
    }
}

impl FromWithBuffer<&Position> for Index {
    fn from_with_buf(pos: &Position, buffer: &Buffer) -> Self {
        let line = buffer.line_bounds(pos.line as usize);
        line.0 + pos.character as usize
    }
}

impl<T> FromWithBuffer<T> for T {
    fn from_with_buf(o: T, _: &Buffer) -> Self {
        o
    }
}

impl Buffer {
    pub fn sorted_completions(&self) -> Vec<&LspCompletion> {
        let cursor_idx = self.cursor().head;
        let before_cursor_idx = cursor_idx.saturating_sub(20);
        let window = self.text_slice(before_cursor_idx..cursor_idx);
        let win_size = window.len();

        let completions = &self.completions;
        completions
            .iter()
            .sorted_by_key(|c| match &c.data {
                CompletionData::Simple(text) => {
                    let chars_len = text.chars().count();
                    for nb in (0..chars_len).rev() {
                        if text.ends_with(&window[(win_size.saturating_sub(nb))..]) {
                            return nb;
                        }
                    }
                    0
                }
                CompletionData::Edit { range, new_text } => {
                    let bounds: Bounds = range.into_with_buf(&self);

                    let buf_text = self.text_slice(bounds.0..bounds.1);
                    if new_text.starts_with(&buf_text) {
                        3
                    } else if new_text.contains(&buf_text) {
                        2
                    } else {
                        1
                    }
                }
            })
            .rev()
            .collect()
    }

    pub fn from_reader<R: Read>(reader: R, src: BufferSource) -> Self {
        Self {
            rope: Rope::from_reader(reader).unwrap(),
            cursor: Cursor { head: 0, tail: 0 },
            version: Default::default(),
            completions: vec![],
            source: src,
        }
    }

    pub fn line_bounds(&self, line: Index) -> Bounds {
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

    pub fn col(&self) -> Index {
        self.col_at(self.cursor().head)
    }

    pub fn row(&self) -> Index {
        self.row_at(self.cursor().head)
    }

    pub fn row_at<I: IntoWithBuffer<Index>>(&self, cur: I) -> Index {
        self.rope.char_to_line(cur.into_with_buf(self))
    }

    pub fn col_at<I: IntoWithBuffer<Index>>(&self, cur: I) -> Index {
        let cur = cur.into_with_buf(self);
        let bounds = self.line_bounds(self.row_at(cur));
        cur - bounds.0
    }

    pub fn move_cursor(&mut self, m: Movement, keep_selection: bool) -> bool {
        let cur_head = self.cursor.head;

        let line = self.row();

        let prev_line = self.line_bounds(line.saturating_sub(1));
        let curr_line = self.line_bounds(line);
        let next_line = self.line_bounds(line.saturating_add(1));

        let max = self.rope.len_chars();
        let new = match m {
            Movement::Up => prev_line.0 + min(prev_line.1 - prev_line.0, cur_head - curr_line.0),
            Movement::Down => {
                if line >= self.rope.len_lines() - 1 {
                    cur_head
                } else {
                    next_line.0 + min(next_line.1 - next_line.0, cur_head - curr_line.0)
                }
            }
            Movement::Left => {
                let next = cur_head.saturating_sub(1);
                if next < curr_line.0 {
                    prev_line.1
                } else {
                    next
                }
            }
            Movement::Right => {
                let next = cur_head.saturating_add(1);
                if next > curr_line.1 {
                    next_line.0
                } else {
                    next
                }
            }
        };

        self.cursor.head = min(new, max);

        if !keep_selection {
            self.cursor.tail = self.cursor.head;
        }

        self.completions = vec![];

        false
    }

    pub fn remove_chars<I: IntoWithBuffer<Bounds>>(&mut self, bounds: I) -> Option<LspInput> {
        let bounds = bounds.into_with_buf(self);

        let mut start = bounds.0;
        let mut end = bounds.1;

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

        let head = self.cursor.head;
        if head >= end {
            self.cursor.head = head - (end - start)
        } else if head >= start {
            self.cursor.head = start
        }

        let tail = self.cursor.tail;
        if tail >= end {
            self.cursor.tail = tail - (end - start)
        } else if tail >= start {
            self.cursor.tail = start
        }

        self.rope.remove(start..end);

        self.lsp_edit()
    }

    pub fn insert<I: IntoWithBuffer<Index>>(&mut self, start: I, chars: &str) -> Option<LspInput> {
        let start = start.into_with_buf(self);

        let curr = self.cursor.head;
        if curr >= start {
            self.cursor.head = curr + chars.chars().count()
        }

        let curr = self.cursor.tail;
        if curr >= start {
            self.cursor.tail = curr + chars.chars().count()
        }

        self.rope.insert(start, chars);

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
        match a {
            Action::Insert(chars) => {
                if self.cursor.head != self.cursor.tail {
                    let bounds = (self.cursor.min(), self.cursor.max());
                    self.remove_chars(bounds);
                }
                self.insert(self.cursor.head, chars.as_str())
            }
            Action::Backspace => {
                if self.cursor.head != self.cursor.tail {
                    self.remove_chars((self.cursor.min(), self.cursor.max()))
                } else {
                    self.remove_chars((self.cursor.head.saturating_sub(1), self.cursor.head))
                }
            }
            Action::Delete => {
                if self.cursor.head != self.cursor.tail {
                    self.remove_chars((self.cursor.min(), self.cursor.max()))
                } else {
                    self.remove_chars((self.cursor.head, self.cursor.head.saturating_add(1)))
                }
            }
        }
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor.clone()
    }

    pub fn text(&self) -> String {
        self.rope.chars().collect()
    }

    pub fn text_slice<R: RangeBounds<usize>>(&self, range: R) -> String {
        self.rope.slice(range).chars().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::buffer::{Action, Buffer, BufferSource, Movement};

    #[test]
    fn selection() {
        let mut buf = Buffer::from_reader(Cursor::new("test"), BufferSource::Text);
        buf.move_cursor(Movement::Right, true);
        buf.move_cursor(Movement::Right, true);
        buf.do_action(Action::Insert("as".into()));
        assert_eq!(buf.cursor.head, buf.cursor.head);
        assert_eq!(buf.text(), "asst")
    }

    #[test]
    fn edit() {
        let mut buf = Buffer::from_reader(Cursor::new("test"), BufferSource::Text);
        buf.insert(1, "yay");
        assert_eq!(buf.text(), "tyayest");
        buf.remove_chars((1, 5));
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
        assert_eq!(b.cursor().head, 0);
        b.move_cursor(Movement::Right, false);
        assert_eq!(b.cursor().head, 1);
        b.move_cursor(Movement::Right, false);
        assert_eq!(b.cursor().head, 2);
        b.move_cursor(Movement::Right, false);
        assert_eq!(b.cursor().head, 3);
        b.move_cursor(Movement::Right, false);
        assert_eq!(b.cursor().head, 4);
        b.move_cursor(Movement::Down, false);
        assert_eq!(b.cursor().head, 8);
        b.move_cursor(Movement::Left, false);
        b.move_cursor(Movement::Up, false);
        assert_eq!(b.cursor().head, 2);
        b.move_cursor(Movement::Up, false);
        assert_eq!(b.cursor().head, 2);
        b.move_cursor(Movement::Left, false);
        b.move_cursor(Movement::Left, false);
        b.move_cursor(Movement::Left, false);
        b.move_cursor(Movement::Left, false);
        b.move_cursor(Movement::Left, false);
        assert_eq!(b.cursor().head, 0);
        b.move_cursor(Movement::Right, false);
        assert_eq!(b.cursor().head, 1);
        b.move_cursor(Movement::Down, false);
        assert_eq!(b.cursor().head, 6);
        b.move_cursor(Movement::Down, false);
        assert_eq!(b.cursor().head, 10);
        b.move_cursor(Movement::Down, false);
        assert_eq!(b.cursor().head, 10);
        b.move_cursor(Movement::Right, false);
        b.move_cursor(Movement::Right, false);
        b.move_cursor(Movement::Right, false);
        b.move_cursor(Movement::Right, false);
        b.move_cursor(Movement::Right, false);
        b.move_cursor(Movement::Right, false);
        b.move_cursor(Movement::Right, false);
        assert_eq!(b.cursor().head, 15);
    }
}
