use rand::Rng;
use std::io::Cursor;
use ste_lib::buffer::{Action, Buffer, Movement};

#[test]
fn fuzz() {
    let mut buffer = Buffer::from_reader(1, Cursor::new(String::new()));

    let mut rng = rand::thread_rng();

    for i in 0..100000 {
        let r = rng.gen_range(0..10);
        match r {
            0 => {
                buffer.do_action(Action::Delete);
                ()
            }
            1 => {
                buffer.do_action(Action::Backspace);
                ()
            }
            2 => {
                let str = rand::thread_rng()
                    .sample_iter::<char, _>(rand::distributions::Standard)
                    .take(2)
                    .collect();
                buffer.do_action(Action::Insert(str));
            }
            3 => {
                buffer.move_cursor(Movement::Left, true);
                ()
            }
            4 => {
                buffer.move_cursor(Movement::Right, true);
                ()
            }
            5 => {
                buffer.move_cursor(Movement::Up, true);
                ()
            }
            6 => {
                buffer.move_cursor(Movement::Down, true);
                ()
            }
            7 => {
                buffer.line_bounds(rng.gen_range(0..50));
                ()
            }
            _ => {
                buffer.do_action(Action::Insert("\n".into()));
                ()
            }
        }
        if i % 1000 == 0 {
            println!("{}", i);
        }
    }
}
