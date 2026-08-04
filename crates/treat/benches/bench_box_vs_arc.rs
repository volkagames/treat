use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;

fn bench_box(c: &mut Criterion) {
    c.bench_function("Box allocation", |b| {
        b.iter(|| {
            let x = Box::new(black_box(42));
            black_box(&x);
        });
    });
}

fn bench_rc(c: &mut Criterion) {
    c.bench_function("Rc allocation", |b| {
        b.iter(|| {
            let x = Rc::new(black_box(42));
            black_box(&x);
        });
    });
}

fn bench_arc(c: &mut Criterion) {
    c.bench_function("Arc allocation", |b| {
        b.iter(|| {
            let x = Arc::new(black_box(42));
            black_box(&x);
        });
    });
}

fn bench_tri_arc(c: &mut Criterion) {
    c.bench_function("triomphe::Arc allocation", |b| {
        b.iter(|| {
            let x = triomphe::Arc::new(black_box(42));
            black_box(&x);
        });
    });
}

criterion_group!(benches, bench_box, bench_rc, bench_arc, bench_tri_arc);
criterion_main!(benches);
