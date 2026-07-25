pub mod emulator;

pub use emulator::Emulator;
// Re-exported so downstream crates can reference `ferrux_term::vt100::Screen`
// without taking their own dependency on the `vt100` crate directly.
pub use vt100;
