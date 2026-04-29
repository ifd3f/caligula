use std::{hint::black_box, sync::Arc, time::Instant};

use bytes::BytesMut;
use bytesize::ByteSize;
use criterion::{Criterion, criterion_group, criterion_main};
use rand::{Rng, RngExt, SeedableRng, rngs::SmallRng};
use stdiomux::frame::*;

const SEED: u64 = 0;
const BASIC_BYTES_TO_TRANSFER: ByteSize = ByteSize::mib(10);
const IO_BYTES_TO_TRANSFER: ByteSize = ByteSize::mib(100);

/// Helper function for generating random data frames.
fn generate_data(seed: u64, bytes: ByteSize) -> Vec<(ChannelId, ChannelDataFrame)> {
    let mut rng = SmallRng::seed_from_u64(seed);
    let count = 2 * bytes.0 / (MAX_PAYLOAD as u64);
    (0..count)
        .map(|_| {
            let id = ChannelId(rng.random());

            let len = rng.random_range(0..=MAX_PAYLOAD);
            let mut buf = BytesMut::zeroed(len);
            rng.fill_bytes(&mut buf);
            (id, ChannelDataFrame(buf.freeze()))
        })
        .collect()
}

fn reads_from_vec(c: &mut Criterion) {
    let payloads = generate_data(SEED, BASIC_BYTES_TO_TRANSFER);
    let mut bytes = vec![];
    for (id, f) in &payloads {
        bytes
            .write_frame(&Frame::ChannelData(*id, f.clone()))
            .unwrap();
    }

    c.bench_function(stringify!(reads_from_vec), |b| {
        b.iter(|| {
            let mut cursor = bytes.as_slice();
            for _ in 0..payloads.len() {
                black_box(&mut cursor).read_frame().unwrap();
            }
        })
    });
}

fn writes_to_vec(c: &mut Criterion) {
    let payloads = generate_data(SEED, BASIC_BYTES_TO_TRANSFER);

    c.bench_function(stringify!(writes_to_vec), |b| {
        b.iter(|| {
            let mut dst = Vec::with_capacity((4 + MAX_PAYLOAD) * payloads.len());

            for (id, f) in &payloads {
                let payload = black_box(Frame::ChannelData(*id, ChannelDataFrame(f.0.clone())));
                dst.write_frame(black_box(&payload)).unwrap();
            }
        })
    });
}

fn reads_from_vec_async(c: &mut Criterion) {
    let payloads = generate_data(SEED, BASIC_BYTES_TO_TRANSFER);
    let mut bytes = vec![];
    for (id, f) in &payloads {
        bytes
            .write_frame(&Frame::ChannelData(*id, f.clone()))
            .unwrap();
    }

    c.bench_function(stringify!(reads_from_vec_async), |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let mut cursor = bytes.as_slice();
                for _ in 0..payloads.len() {
                    black_box(&mut cursor).read_frame_async().await.unwrap();
                }
            })
    });
}

fn writes_to_vec_async(c: &mut Criterion) {
    let payloads = generate_data(SEED, BASIC_BYTES_TO_TRANSFER);

    c.bench_function(stringify!(writes_to_vec_async), |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let mut dst = Vec::with_capacity((4 + MAX_PAYLOAD) * payloads.len());

                for (id, f) in &payloads {
                    let payload = black_box(Frame::ChannelData(*id, ChannelDataFrame(f.0.clone())));
                    dst.write_frame_async(black_box(&payload)).await.unwrap();
                }
            })
    });
}

fn writes_to_dev_null(c: &mut Criterion) {
    let payloads = generate_data(SEED, IO_BYTES_TO_TRANSFER);

    c.bench_function(stringify!(writes_to_dev_null), |b| {
        b.iter(|| {
            let mut dst = std::fs::File::create("/dev/null").unwrap();

            for (id, f) in &payloads {
                let payload = black_box(Frame::ChannelData(*id, f.clone()));
                dst.write_frame(black_box(&payload)).unwrap();
            }
        })
    });
}

fn send_over_pipe(c: &mut Criterion) {
    c.bench_function(stringify!(send_over_pipe), |b| {
        b.iter_custom(|iter| {
            let (mut rx, mut tx) = std::io::pipe().unwrap();
            let payloads = generate_data(iter, IO_BYTES_TO_TRANSFER);

            let start_barrier = Arc::new(std::sync::Barrier::new(3));
            let end_barrier = Arc::new(std::sync::Barrier::new(3));
            let times = payloads.len();

            let writer = std::thread::spawn({
                let start_barrier = start_barrier.clone();
                let end_barrier = end_barrier.clone();
                move || {
                    start_barrier.wait();
                    for (id, f) in payloads {
                        tx.write_frame(black_box(&Frame::ChannelData(id, f)))
                            .unwrap();
                    }
                    end_barrier.wait();
                }
            });

            let reader = std::thread::spawn({
                let start_barrier = start_barrier.clone();
                let end_barrier = end_barrier.clone();
                move || {
                    start_barrier.wait();
                    for _ in 0..times {
                        rx.read_frame().unwrap();
                    }
                    end_barrier.wait();
                }
            });

            let start_time = Instant::now();
            start_barrier.wait();
            end_barrier.wait();
            let duration = start_time.elapsed();

            reader.join().unwrap();
            writer.join().unwrap();

            duration
        })
    });
}

fn send_over_pipe_async(c: &mut Criterion) {
    c.bench_function(stringify!(send_over_pipe), |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_custom(|iter| async move {
                let (mut rx, mut tx) = tokio_pipe::pipe().unwrap();
                let payloads = generate_data(iter, IO_BYTES_TO_TRANSFER);

                let start_barrier = Arc::new(tokio::sync::Barrier::new(3));
                let end_barrier = Arc::new(tokio::sync::Barrier::new(3));
                let times = payloads.len();

                let writer = tokio::spawn({
                    let start_barrier = start_barrier.clone();
                    let end_barrier = end_barrier.clone();
                    async move {
                        start_barrier.wait().await;
                        for (id, f) in payloads {
                            tx.write_frame_async(black_box(&Frame::ChannelData(id, f)))
                                .await
                                .unwrap();
                        }
                        end_barrier.wait().await;
                    }
                });

                let reader = tokio::spawn({
                    let start_barrier = start_barrier.clone();
                    let end_barrier = end_barrier.clone();
                    async move {
                        start_barrier.wait().await;
                        for _ in 0..times {
                            rx.read_frame_async().await.unwrap();
                        }
                        end_barrier.wait().await;
                    }
                });

                let start_time = Instant::now();
                start_barrier.wait().await;
                end_barrier.wait().await;
                let duration = start_time.elapsed();

                reader.await.unwrap();
                writer.await.unwrap();

                duration
            })
    });
}

criterion_group!(
    name = basic;
    config = Criterion::default();
    targets = reads_from_vec, writes_to_vec,
);
criterion_group!(
    name = basic_async;
    config = Criterion::default();
    targets = reads_from_vec_async, writes_to_vec_async,
);
criterion_group!(
    name = io;
    config = Criterion::default().sample_size(20);
    targets = writes_to_dev_null, send_over_pipe,
);
criterion_group!(
    name = io_async;
    config = Criterion::default().sample_size(20);
    targets = send_over_pipe_async,
);
criterion_main!(basic, basic_async, io, io_async);
