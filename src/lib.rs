use std::sync::{
    mpsc::{channel, Receiver, Sender},
    Mutex,
};
use std::{ops::Deref, sync::Arc};

type Buf<T> = Arc<T>;
struct ReadUpdate<T> {
    shared: Arc<Mutex<Option<Buf<T>>>>,
}
impl<T> ReadUpdate<T> {
    fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(None)),
        }
    }
    fn replace(&self, v: Buf<T>) -> Option<Buf<T>> {
        std::mem::replace(&mut self.shared.lock().unwrap(), Some(v))
    }
    fn get(&self) -> Option<Buf<T>> {
        self.shared.lock().unwrap().take()
    }
}

pub struct Writer<T> {
    make_buf: Box<dyn FnMut(&T) -> T>,
    unused_bufs_rx: Receiver<Buf<T>>,

    prev_buf: Buf<T>,
    unused_bufs_tx: Sender<Buf<T>>,
    read_update: ReadUpdate<T>,
}

pub struct Reader<T> {
    prev_buf: Buf<T>,
    unused_bufs_tx: Sender<Buf<T>>,
    read_update: ReadUpdate<T>,
}

pub fn new_buffer_with<T>(
    init: T,
    make_buf: impl FnMut(&T) -> T + 'static,
) -> (Writer<T>, Reader<T>) {
    let w = Writer::new(init, make_buf);
    let r = Reader {
        prev_buf: w.prev_buf.clone(),
        unused_bufs_tx: w.unused_bufs_tx.clone(),
        read_update: ReadUpdate {
            shared: w.read_update.shared.clone(),
        },
    };
    (w, r)
}

pub fn new_buffer_clone<T: Clone>(init: T) -> (Writer<T>, Reader<T>) {
    new_buffer_with(init, |v| v.clone())
}

impl<T> Writer<T> {
    fn new(init: T, make_buf: impl FnMut(&T) -> T + 'static) -> Self {
        let prev_buf = Arc::new(init);
        let make_buf = Box::new(make_buf);
        let read_update = ReadUpdate::new();
        let (unused_bufs_tx, unused_bufs_rx) = channel();
        Self {
            prev_buf,
            make_buf,
            unused_bufs_tx,
            unused_bufs_rx,
            read_update,
        }
    }

    fn get_unused_buffer(&mut self) -> Buf<T> {
        if let Some(buf) = self.unused_bufs_rx.try_recv().ok() {
            debug_assert!(Arc::strong_count(&buf) == 1);
            debug_assert!(Arc::weak_count(&buf) == 0);
            return buf;
        }
        let new_state = (self.make_buf)(&self.prev_buf);
        Arc::new(new_state)
    }

    pub fn update(&mut self, mut write_op: impl FnMut(&T, &mut T)) {
        // This Arc will have no other clones at this point,
        // so we can get a mutable reference into it.
        let mut new_state = self.get_unused_buffer();

        let mut_ref = Arc::get_mut(&mut new_state).unwrap();
        write_op(&self.prev_buf, mut_ref);

        self.prev_buf = new_state.clone();
        if let Some(unused_buf) = self.read_update.replace(new_state) {
            self.unused_bufs_tx.send(unused_buf).unwrap();
        }
    }
}

impl<T> Reader<T> {
    pub fn newest(&mut self) -> ReadState<'_, T> {
        match self.read_update.get() {
            Some(new_buf) => {
                let now_unused_buf = std::mem::replace(&mut self.prev_buf, new_buf);
                self.unused_bufs_tx.send(now_unused_buf).unwrap();
                ReadState {
                    buf: &self.prev_buf,
                }
            }
            None => ReadState {
                buf: &self.prev_buf,
            },
        }
    }
}

pub struct ReadState<'a, T> {
    buf: &'a Buf<T>,
}

impl<T> Deref for ReadState<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure() -> [Arc<Mutex<usize>>; 2] {
        let p = Arc::new(Mutex::new(0));
        [p.clone(), p]
    }

    fn count(ptr: &Arc<Mutex<usize>>) {
        *ptr.lock().unwrap() += 1;
    }

    fn final_count(ptr: &Arc<Mutex<usize>>) -> usize {
        *ptr.lock().unwrap()
    }

    #[test]
    fn test_seq_1() {
        let [c, c2] = measure();

        let (mut w, mut r) = new_buffer_with(0, move |i| {
            count(&c2);
            *i
        });
        assert_eq!(*r.newest(), 0);
        w.update(|old, new| {
            *new = *old + 1;
        });
        assert_eq!(*r.newest(), 1);
        assert!(final_count(&c) <= 2);
    }

    #[test]
    fn test_long_overlapping_read() {
        let [c, c2] = measure();

        let (mut w, mut r) = new_buffer_with(0, move |i| {
            count(&c2);
            *i
        });
        {
            let r = r.newest();
            assert_eq!(*r, 0);
            w.update(|old, new| {
                *new = *old + 1;
            });
            assert_eq!(*r, 0);
            w.update(|old, new| {
                *new = *old + 1;
            });
            assert_eq!(*r, 0);
            w.update(|old, new| {
                *new = *old + 1;
            });
            assert_eq!(*r, 0);
            w.update(|old, new| {
                *new = *old + 1;
            });
            assert_eq!(*r, 0);
            w.update(|old, new| {
                *new = *old + 1;
            });
            assert_eq!(*r, 0);
        }
        assert_eq!(*r.newest(), 5);
        assert!(final_count(&c) <= 2);
    }

    #[test]
    fn test_long_overlapping_write() {
        let [c, c2] = measure();

        let (mut w, mut r) = new_buffer_with(0, move |i| {
            count(&c2);
            *i
        });

        w.update(|old, new| {
            assert_eq!(*r.newest(), 0);
            assert_eq!(*r.newest(), 0);
            assert_eq!(*r.newest(), 0);
            assert_eq!(*r.newest(), 0);
            assert_eq!(*r.newest(), 0);
            *new = *old + 1;
        });
        assert_eq!(*r.newest(), 1);

        assert!(final_count(&c) <= 2);
    }
}
