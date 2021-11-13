use ropey::Rope;
use std::cell::Cell;
use std::cmp::min;
use std::io::Read;

pub struct Buffer {
    rope: Rope,
    cursor: Cell<usize>,
}

impl Buffer {
    pub fn from_reader<R: Read>(reader: R) -> Self {
        Self {
            rope: Rope::from_reader(reader).unwrap(),
            cursor: Cell::new(0),
        }
    }
}

pub enum Movement {
    Up,
    Down,
    Left,
    Right,
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

        if end > start + 1 {
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

    fn current_line(&self) -> usize {
        self.rope.char_to_line(self.cursor.get())
    }

    pub fn move_cursor(&self, m: Movement) {
        let cur = self.cursor.get();
        let line = self.current_line();

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
            Movement::Left => cur.saturating_sub(1),
            Movement::Right => cur.saturating_add(1),
        };

        self.cursor.set(min(new, max));
    }

    pub fn cursor(&self) -> usize {
        self.cursor.get()
    }
}

#[cfg(test)]
mod tests {
    use crate::buffer::{Buffer, Movement};
    use std::io::Cursor;

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
