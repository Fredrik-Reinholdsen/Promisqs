//! Promisqs (pronounced "promiscuous") is a cross-platform, shared memory,
//! multi-producer, multi-consumer queue implementation.
//!
//! The main use-case for this library is for ultra low latency inter-process communication (IPC),
//! significantly faster than you would get using any other means, like sockets, UNIX sockets (named pipes
//! on windows) and even things like DBUS on Linux.
//! For smaller sized types (a few kB or smaller), Promisqs can easily achieve a latency of a few tenths of a micro-second!
//!
//! The queue implementation [`ShmemQueue`](queue/struct.ShmemQueue.html) is generic, allowing most primitive types, and any type that implements the [`FromBytes`](https://docs.rs/zerocopy/latest/zerocopy/trait.FromBytes.html),
//! [`IntoBytes`](https://docs.rs/zerocopy/latest/zerocopy/trait.IntoBytes.html), and [`Immutable`](https://docs.rs/zerocopy/latest/zerocopy/trait.Immutable.html) traits, to be used with the queue.
//!
//! Note that these traits cannot be implemented directly for a type, and must be directly derived, due to safety concerns.
//! Most primitive types and other fixed size types, like enums, arrays etc.
//! can be used with the queue. Dynamic types like strings or vectors cannot.
//!
//! # Producer Example
//!
//! ```
//! use promisqs::{ShmemQueue, IntoBytes, FromBytes, Immutable};
//!
//! // Define a struct that derives zerocopy traits,
//! // so that we can use it with promisqs queues
//! #[derive(Debug, Clone, IntoBytes, FromBytes, Immutable, PartialEq)]
//! pub struct Message {
//!     id: u8,
//!     address: [u8; 4],
//!     payload: [u8; 4],
//! }
//!
//! let mut q = ShmemQueue::<Message>::create("flink.map", 1).unwrap();
//!
//! let msg = Message {
//!     id: 0x0,
//!     address: [0xc8, 0xa8, 0x1, 0x1],
//!     payload: [01, 0x2, 0x3, 0x4],
//! };
//!
//! q.push(&msg).unwrap();
//!
//! # drop(q);
//! # std::thread::sleep(std::time::Duration::from_millis(5000));
//! ```
//! # Consumer Example (different process)
//! ```
//! use promisqs::{ShmemQueue, IntoBytes, FromBytes, Immutable};
//!
//! #[derive(Debug, Clone, IntoBytes, FromBytes, Immutable, PartialEq)]
//! pub struct Message {
//!     id: u8,
//!     address: [u8; 4],
//!     payload: [u8; 4],
//! }
//!
//! # let _q = ShmemQueue::<Message>::create("flink.map", 1).unwrap();
//! let mut q = ShmemQueue::<Message>::open("flink.map").unwrap();
//!
//! let expected_msg = Message {
//!     id: 0x0,
//!     address: [0xc8, 0xa8, 0x1, 0x1],
//!     payload: [01, 0x2, 0x3, 0x4],
//! };
//! # q.push(&expected_msg).unwrap();
//!
//! assert_eq!(q.pop().unwrap(), expected_msg);
//!
//! # drop(_q);
//! # drop(q);
//! # std::thread::sleep(std::time::Duration::from_millis(5000));
//! ```

pub mod error;
pub mod queue;

pub use error::PromisqsError;
pub use queue::{PromisqsResult, ShmemQueue};

pub use zerocopy::{FromBytes, Immutable, IntoBytes};
