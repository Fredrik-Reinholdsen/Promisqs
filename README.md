# Promisqs 
Promisqs (pronounced _promiscuous_) provides a generic multi-producer, multi-consumer (MPMC) shared memory queue implementation in Rust.
The queue implementation is lock-free, but not necessarily wait free, since it is based around atomics.
However, in the case that there is only a single producer, writes are guaranteed wait-free,
and if there is only a single consumer then reads are also guaranteed wait-free.

Used for very low-latency and high throughput inter-process communication (IPC).
The latency depends on your CPU and the element size, but for small element sizes (<10 kB),
latency is typically well into the sub micro-second range (around 0.3 micro-seconds on my PC).
This is roughly is 2-3 times faster than D-Bus, and more than an order of magnitude faster Unix-sockets (50-60 times).

## Safety
Since the shared memory queue is usually accessed from a whole different process/program than the one that instantiated it,
the compiler cannot make any guarantees about the type of the queue elements being what you think it is. 
So it is up to *you* the programmer to ensure that the type is correct when opening a shared memory que

When attaching to an already open queue, the size of the defined element type is compared to the size of elements in the queue.
This may catch some errors, like for example attaching to a queue with a defined type of `f64`, but the queue elements are actually `f32`.
However, this will for example not catch attaching to a queue with the defined type of `f32`, but the queue elements are actually `u32`.
In this or in similar case no errors will be raised, and you will misinterpret the queue data.
