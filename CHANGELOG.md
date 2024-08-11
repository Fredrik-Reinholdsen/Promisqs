## 0.2.0
Changes to underlying queue implementation, added new tests, and new queue methods.

## Changes
- Change to the underlying queue implementation for fault tolerance under heavy loads.

## Added
- Added `try_push` method, that will attempt to push to the queue, in a non-blocking manner. If another thread/process makes it first then this method will fail instead of blocking, meaning this is a fixed time function.
- Added `try_pop` method, that will attempt to pop from the queue, in a non-blocking manner. If another thread/process makes it first then this method will fail instead of blocking, meaning this is a fixed time function.
- Added several new tests, including multi-threaded smoke tests to test robustness.
- Added new `WouldBlock` error for when the `try_push` and `try_pop` methods would block.

