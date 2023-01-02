use promisqs::queue::ShmemQueue;
use std::thread;
use std::time::Instant;

fn main() {
    let mut q = ShmemQueue::create("test.map", 10).unwrap();
    let v1 = [1_u8; 1024];
    let v2 = [2_u8; 1024];
    q.push(&v1).unwrap();
    q.push(&v2).unwrap();
    assert_eq!(v1, q.pop().unwrap());
    assert_eq!(v2, q.pop().unwrap());
    let t0 = Instant::now();
    let mut cnt = 0;
    let v = [1_u8; 1024];
    while t0.elapsed().as_secs() < 5 {
        q.push(&v).unwrap();
        q.pop().unwrap();
        cnt += 1;
    }
    println!("Performed {} MOps/sec", cnt as f64 * 2.0 * 1e-6);
}
