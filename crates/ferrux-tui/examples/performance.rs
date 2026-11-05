//! Deterministic in-process workloads; run repeatedly with --release.
use std::hint::black_box;
use std::time::Instant;

use ferrux_ipc::{read_message, write_message, Request};
use ferrux_term::Emulator;
use ferrux_tui::widgets::pane_view::render_screen;

fn main() {
    let line = "\x1b[32magent\x1b[0m: compiling 界 e\u{301} output\r\n";
    let workload = line.repeat(4096);
    let mut emulator = Emulator::new(40, 120, 1000);
    let start = Instant::now();
    for _ in 0..64 {
        emulator.process(black_box(workload.as_bytes()));
    }
    println!(
        "parse_bytes={},parse_ns={}",
        workload.len() * 64,
        start.elapsed().as_nanos()
    );
    let start = Instant::now();
    for _ in 0..2000 {
        black_box(render_screen(black_box(emulator.screen())));
    }
    println!(
        "render_frames=2000,render_ns={}",
        start.elapsed().as_nanos()
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let start = Instant::now();
        for _ in 0..10000 {
            let mut frame = Vec::new();
            write_message(
                &mut frame,
                &Request::ResizePane {
                    id: 1,
                    cols: 120,
                    rows: 40,
                },
            )
            .await
            .unwrap();
            let request: Request = read_message(&mut frame.as_slice()).await.unwrap();
            black_box(request);
        }
        println!("ipc_roundtrips=10000,ipc_ns={}", start.elapsed().as_nanos());
    });
}
