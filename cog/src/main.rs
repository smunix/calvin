use calvin_storage::fregion::FRegion;
use calvin_storage::ring::ShmRing;
use clap::Parser;
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about = "Calvin structured storage recorder daemon", long_about = None)]
struct Args {
    /// The storage group to listen to
    #[arg(short, long)]
    group: String,

    /// The output fregion file path
    #[arg(short, long, default_value = "out.log")]
    out: String,
}

fn main() {
    let args = Args::parse();

    let ring_path = format!("/tmp/calvin_ring_{}", args.group);

    let ring = match ShmRing::open(&ring_path) {
        Ok(r) => r,
        Err(_) => ShmRing::create(&ring_path, 1024 * 1024).expect("Failed to create ring buffer"),
    };

    let mut _fregion = FRegion::open(&args.out).unwrap_or_else(|_| {
        FRegion::create(&args.out, 1024 * 1024 * 10).expect("Failed to create fregion")
    });

    println!("Cog listening on group: {}", args.group);
    println!("Recording to: {}", args.out);

    let mut buf = vec![0u8; 1024 * 1024];

    loop {
        let read = ring.pop(&mut buf);
        if read > 0 {
            println!("Recorded {} bytes", read);
        } else {
            thread::sleep(Duration::from_millis(1));
        }
    }
}
