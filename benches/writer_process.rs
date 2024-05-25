use std::{
    io::{Cursor, Read},
    time::Instant,
};

use caligula::bench::{compression::CompressionFormat, writer_process_utils::FileSourceReader};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::{rngs::SmallRng, Rng, SeedableRng};

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("FileSourceReader", |b| {
        b.iter_custom(|_| {
            let read_size = 50usize * (1 << 20); // 50MiB
            let buf_size = 512usize * (1 << 10); // 512KiB

            let mut rng = SmallRng::from_entropy();
            let random_bytes: Vec<u8> = (0..read_size).map(|_| rng.gen()).collect();
            let reader = Cursor::new(&random_bytes);

            let mut reader =
                FileSourceReader::new(CompressionFormat::Identity, buf_size, black_box(reader));
            let mut buf = vec![0u8; buf_size];

            let start = Instant::now();
            loop {
                match reader.read(&mut buf).unwrap() {
                    0 => break,
                    _ => continue,
                }
            }
            let elapsed = start.elapsed();

            // prevent compiler from optimizing these counts away
            black_box(reader.read_file_bytes());
            black_box(reader.decompressed_bytes());

            elapsed
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
