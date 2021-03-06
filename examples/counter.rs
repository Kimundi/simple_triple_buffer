#[derive(Clone)]
struct State {
    v: u64,
}

use std::time::Duration;

use simple_triple_buffer::new_with;

fn main() {
    let (mut w, mut r) = new_with(State { v: 0 }, |s| {
        println!("Cloned state!");
        s.clone()
    });

    let tw = std::thread::spawn(move || loop {
        w.write_new(|last, new| {
            new.v = last.v + 1;
        });
    });

    let tr = std::thread::spawn(move || {
        let mut last = 0;
        loop {
            let state = r.read_newest();
            println!("Value: {} (+{})", state.v, state.v - last);
            last = state.v;
            std::thread::sleep(Duration::from_millis(20));
        }
    });

    tw.join().unwrap();
    tr.join().unwrap();
}
