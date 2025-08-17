# Promisqs 
Promisqs (pronounced _promiscuous_) provides a generic multi-producer, multi-consumer (MPMC), lock-free shared memory queue implementation in Rust.

Promisqs queues are useful for ultra low-latency and high throughput inter-process communication (IPC).
The latency depends on your CPU and the element size, but for small element sizes (<10 kB),
latency is typically well into the sub micro-second range (around 0.3 micro-seconds on my PC).
This is roughly is 2-3 times faster than D-Bus, more than an order of magnitude faster Unix-sockets (50-60 times), and multiple orders of magnitude (100-200) times faster than a loopback TCP connection.

Promisqs queues also offer very high throughputs, especially compared to TCP sockets.

## Safety
Since the shared memory queue is usually accessed from a whole different process/program than the one that instantiated it,
the compiler cannot make any guarantees about the type of the queue elements being what you think it is. 
So it is up to *you* the programmer to ensure that the type is correct when opening a shared memory queue

When attaching to an already open queue, the size of the defined element type is compared to the size of elements in the queue.
This may catch some errors, like for example attaching to a queue with a defined type of `f64`, but the queue elements are actually `f32`.
However, this will for example not catch attaching to a queue with the defined type of `f32`, but the queue elements are actually `u32`.
In this or in similar case no errors will be raised, and you will misinterpret the queue data.

## Usage
```toml
[dependencies]
promisqs = "1.1"
zerocopy = "0.8"
```

Below is a small example for passing messages of a custom type.

### Producer

```rust
use promisqs::{ShmemQueue, IntoBytes, FromBytes, Immutable};

// Define a struct that derives zerocopy traits,
// so that we can use it with promisqs queues
#[derive(Debug, Clone, IntoBytes, FromBytes, Immutable, PartialEq)]
pub struct Message {
    id: u8,
    address: [u8; 4],
    payload: [u8; 4],
}

let mut q = ShmemQueue::<Message>::create("flink.map", 1).unwrap();

let msg = Message {
    id: 0x0,
    address: [0xc8, 0xa8, 0x1, 0x1],
    payload: [01, 0x2, 0x3, 0x4],
};

q.push(&msg).unwrap();
```

### Consumer
```rust
use promisqs::{ShmemQueue, IntoBytes, FromBytes, Immutable};

#[derive(Debug, Clone, IntoBytes, FromBytes, Immutable, PartialEq)]
pub struct Message {
    id: u8,
    address: [u8; 4],
    payload: [u8; 4],
}

let mut q = ShmemQueue::<Message>::open("flink.map").unwrap();

let expected_msg = Message {
    id: 0x0,
    address: [0xc8, 0xa8, 0x1, 0x1],
    payload: [01, 0x2, 0x3, 0x4],
};

assert_eq!(q.pop().unwrap(), expected_msg);
```

## License

Licensed under
 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
