use promisqs::ShmemQueue;
use std::time::Duration;

// Create a queue, push some objects and see
// that they are popped in the same order that they are pushed
#[test]
fn test_queue() {
    let mut q = ShmemQueue::create("./test_queue.map", 10).unwrap();
    assert!(std::path::Path::new("./test_queue.map").exists());
    let v1 = [1_u8; 1024];
    let v2 = [2_u8; 1024];
    q.push(&v1).unwrap();
    q.push(&v2).unwrap();
    assert_eq!(v1, q.pop().unwrap());
    assert_eq!(v2, q.pop().unwrap());
    drop(q);
    std::thread::sleep(Duration::from_millis(1));
    assert!(!std::path::Path::new("./test_queue.map").exists());
}

// Test that we get errors when pushing to a full queue
// and popping from an empty queue
#[test]
fn test_bounds() {
    let capacity = 10;
    let mut q = ShmemQueue::create("test_bounds.map", capacity).unwrap();
    assert!(q.pop().is_err());
    let v1 = [255_u8; 1024];
    for _ in 0..capacity {
        q.push(&v1).unwrap();
    }

    assert!(q.push(&v1).is_err());
}

// Test that the values are read back correctly when the write and read
// pointer wrap around to the start of the buffer
#[test]
fn test_wrapping() {
    let capacity = 10;
    let mut q = ShmemQueue::create("test_wrapping.map", capacity).unwrap();
    let v1 = [255_u8; 1024];
    for _ in 0..capacity {
        q.push(&v1).unwrap();
    }
    assert!(q.pop().is_ok());
    let v2 = [133_u8; 1024];
    assert!(q.push(&v2).is_ok());
    for _ in 0..(capacity - 1) {
        assert_eq!(v1, q.pop().unwrap());
    }
    assert_eq!(v2, q.pop().unwrap());
}
