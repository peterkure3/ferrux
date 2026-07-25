use std::io;

pub trait IpcTransport {
    fn send(&self, payload: &[u8]) -> io::Result<()>;
    fn recv(&self) -> io::Result<Vec<u8>>;
}
