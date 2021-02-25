use std::sync::Mutex;
use std::{ops::Deref, sync::Arc};

type Buf<T> = Arc<T>;
struct ReadUpdate<T> {
    shared: Arc<Mutex<Buf<T>>>,
}
impl<T> ReadUpdate<T> {
    fn new(v: Buf<T>) -> Self {
        Self {
            shared: Arc::new(Mutex::new(v)),
        }
    }
    fn replace(&self, v: Buf<T>) -> Buf<T> {
        std::mem::replace(&mut self.shared.lock().unwrap(), v)
    }
    fn get(&self) -> Buf<T> {
        self.shared.lock().unwrap().clone()
    }
}

pub struct Writer<T> {
    prev_buf: Buf<T>,
    make_buf: Box<dyn FnMut(&T) -> T>,
    unused_bufs: Vec<Buf<T>>,
    read_update: ReadUpdate<T>,
}

pub struct Reader<T> {
    read_update: ReadUpdate<T>,
}

pub fn new_buffer<T>(init: T, make_buf: impl FnMut(&T) -> T + 'static) -> (Writer<T>, Reader<T>) {
    let w = Writer::new(init, make_buf);
    let r = Reader {
        read_update: ReadUpdate {
            shared: w.read_update.shared.clone(),
        },
    };
    (w, r)
}

impl<T> Writer<T> {
    fn new(init: T, make_buf: impl FnMut(&T) -> T + 'static) -> Self {
        let prev_buf = Arc::new(init);
        let make_buf = Box::new(make_buf);
        let read_update = ReadUpdate::new(prev_buf.clone());
        Self {
            prev_buf,
            make_buf,
            unused_bufs: Vec::new(),
            read_update,
        }
    }

    fn get_unused_buffer(&mut self) -> Buf<T> {
        if let Some(buf) = self.unused_bufs.pop() {
            return buf;
        }
        let new_state = (self.make_buf)(&self.prev_buf);
        let buf = Arc::new(new_state);
        debug_assert!(Arc::strong_count(&buf) == 1);
        debug_assert!(Arc::weak_count(&buf) == 0);
        buf
    }

    pub fn update(&mut self, mut write_op: impl FnMut(&T, &mut T)) {
        // This Arc will have no other clones at this point,
        // so we can get a mutable reference into it.
        let mut new_state = self.get_unused_buffer();

        let mut_ref = Arc::get_mut(&mut new_state).unwrap();
        write_op(&self.prev_buf, mut_ref);

        let unused_old_read = self.read_update.replace(new_state.clone());
        self.prev_buf = new_state;
        self.unused_bufs.push(unused_old_read);
    }
}

impl<T> Reader<T> {
    pub fn get_newest(&self) -> ReadState<T> {
        ReadState {
            buf: self.read_update.get(),
        }
    }
}

pub struct ReadState<T> {
    buf: Buf<T>,
}

impl<T> Deref for ReadState<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}
