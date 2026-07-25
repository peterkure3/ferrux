use std::io;

pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
}

pub trait PtyBackend {
    type Handle;

    fn spawn(&self, cmd: &str, size: PtySize) -> io::Result<Self::Handle>;
    fn resize(&self, handle: &Self::Handle, size: PtySize) -> io::Result<()>;
    fn kill(&self, handle: &Self::Handle) -> io::Result<()>;
}
