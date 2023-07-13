# Promisqs 
This crate provides a generic multi-producer, multi-consumer (MPMC) shared memory queue implementation in Rust.
The queue implementation is lock-free, but not necessarily wait free.
However, in the case that there is only a single producer, writes are wait-free,
and if there is only a single consumer then reads are also wait-free.

May be used for very low-latency and high throughput inter-process communication (IPC).
The latency depends on your CPU and the element size, but for small element sizes (<1 kB),
latency is typically well into the sub micro-second range, which is 2-3 times faster than D-Bus,
and more than an order of magnitude faster Unix-sockets (50-60 times).

## Safety
Since the shared memory queue may be accessed from a whole different process/program
than the one that instantiated it, the compiler cannot make any guarantees about
type of queue elements.

When attaching to an already open queue, the size of the defined element type is compared to the size of elements in the queue.
This may catch some errors, like for example attaching to a queue with a defined type of f64, but the queue elements are acqtually f32.
However, this will for example not catch attaching to a queue with the defined type of f32, but the queue elements are actually u32.
In this case no errors will be raised, and you will misinterpret the data.
