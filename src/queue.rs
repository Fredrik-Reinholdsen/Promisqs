use crate::error::PromisqsError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use shared_memory as shmem;
use zerocopy::{AsBytes, FromBytes};

pub type PromisqsResult<T> = Result<T, PromisqsError>;

// Struct that details the layout of the head of the shared memory buffer
// NOTE Not meant to be instatiatied directly.
#[repr(C, align(8))]
#[derive(Debug)]
struct SharedMemory {
    // Flag that signifies if the shared memory queue has been initialized
    is_init: AtomicBool,
    // Write lock
    lock: AtomicBool,
    ref_cnt: AtomicU64,
    // Serialized size of each queue element in bytes
    element_size: usize,
    capacity: usize,
    // Offset from where the next data should be read
    head: usize,
    // Offset from where the next data should be inserted
    // Thus the length of the queue is end - head
    end: usize,
}

pub struct ShmemQueue<'q, T: AsBytes + FromBytes> {
    // Marks the type of the queue
    _marker: std::marker::PhantomData<T>,
    // Memmap shared memory object
    mmap: shmem::Shmem,
    // Size of the shared memory portion used for the queue
    data_size: u64,
    // A reference to the header data in shared memory
    shmem: &'q mut SharedMemory,
    // Pointer to the base of the queue
    data_ptr: *mut T,
    // The size of each element of the queue in bytes
    element_size: usize,
}

unsafe impl<'q, T: Send + AsBytes + FromBytes> Send for ShmemQueue<'q, T> {}
unsafe impl<'q, T: Send + AsBytes + FromBytes> Sync for ShmemQueue<'q, T> {}

// Dropping decrements the sender reference counter.
// If the instance to be dropped is the last sender or receiver
// then the shared memory will be dropped to
impl<'q, T: AsBytes + FromBytes> Drop for ShmemQueue<'q, T> {
    fn drop(&mut self) {
        let n_ref = self.shmem.ref_cnt.fetch_sub(1, Ordering::SeqCst);
        if n_ref == 1 {
            // Take ownership of the mmap to ensure it gets dropped correctly
            self.mmap.set_owner(true);
        }
    }
}

// On clone we must increase the Atomic Referece Counter
// in shared memory
impl<'q, T: AsBytes + FromBytes> Clone for ShmemQueue<'q, T> {
    fn clone(&self) -> Self {
        let mmap_clone = shmem::ShmemConf::new()
            .flink(self.mmap.get_flink_path().unwrap())
            .open()
            .unwrap();
        let shmem_clone = unsafe { &mut *(mmap_clone.as_ptr() as *mut _ as *mut SharedMemory) };

        self.shmem.ref_cnt.fetch_add(1, Ordering::Release);
        Self {
            mmap: mmap_clone,
            shmem: shmem_clone,
            data_ptr: self.data_ptr.clone(),
            element_size: self.element_size.clone(),
            data_size: self.data_size.clone(),
            _marker: self._marker,
        }
    }
}

impl<'q, T: AsBytes + FromBytes> ShmemQueue<'q, T> {
    // Create a new shared memory queue from a capacity and flink
    pub fn create(flink: &str, capacity: usize) -> PromisqsResult<Self> {
        // Calculate the size of T in bytes and the size memory required
        let t_size = std::mem::size_of::<T>();
        let buf = vec![0_u8; t_size];
        let element_size = unsafe { (*(&buf as *const _ as *const T)).as_bytes().len() };
        let data_size = element_size * capacity;
        let head_size = std::mem::size_of::<SharedMemory>();
        let mmap = shmem::ShmemConf::new()
            .size((data_size + head_size) as usize)
            .flink(flink)
            .create()?;
        let ptr = mmap.as_ptr();
        // Initialize the shared memory atomics
        let shmem = unsafe { &mut *(ptr as *const _ as *mut SharedMemory) };
        shmem.ref_cnt.store(1, Ordering::Relaxed);
        shmem.element_size = element_size;
        shmem.capacity = capacity;
        shmem.head = 0;
        shmem.end = 0;
        shmem.is_init.store(true, Ordering::SeqCst);

        let data_ptr = unsafe { ptr.add(head_size as usize) as *mut _ as *mut T };

        let s = Self {
            _marker: std::marker::PhantomData,
            data_size: data_size as u64,
            mmap,
            data_ptr,
            shmem,
            element_size,
        };
        // Mark the shared memory as initialized
        Ok(s)
    }
    /// Open and link up to an already created shared memory queue using file
    pub fn open(flink: &str) -> PromisqsResult<Self> {
        let mmap = shmem::ShmemConf::new().flink(flink).open()?;
        let ptr = mmap.as_ptr();
        let shmem = unsafe { &mut *(ptr as *mut _ as *mut SharedMemory) };
        if shmem.is_init.load(Ordering::Acquire) != true {
            return Err(PromisqsError::BufferNotInitialized);
        }
        shmem.ref_cnt.fetch_add(1, Ordering::Release);

        // Verify that the serialized size of T matches the shmem queue
        // NOTE this is not a guarantee that they are the same type
        // for example f32 and u32 are both the same size, and there is no way
        // for the other process to verify that the types are actually the same
        let t_size = std::mem::size_of::<T>();
        let buf = vec![0_u8; t_size];
        let element_size = unsafe { (*(&buf as *const _ as *const T)).as_bytes().len() };
        if element_size != shmem.element_size {
            return Err(PromisqsError::ElementSizeMismatch);
        }

        let data_size = element_size as usize * shmem.capacity;
        let head_size = std::mem::size_of::<SharedMemory>();
        let data_ptr = unsafe { ptr.add(head_size) as *mut _ as *mut T };

        let s = Self {
            mmap,
            data_size: data_size as u64,
            data_ptr,
            shmem,
            element_size,
            _marker: std::marker::PhantomData,
        };
        Ok(s)
    }

    fn lock(&self) {
        while let Err(_) = self.try_lock() {}
    }

    fn unlock(&self) -> Result<(), PromisqsError> {
        match self.shmem.lock.compare_exchange_weak(
            true,
            false,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(true) => return Ok(()),
            _ => return Err(PromisqsError::LockNotAcquired),
        }
    }

    fn try_lock(&self) -> PromisqsResult<()> {
        match self.shmem.lock.compare_exchange_weak(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(false) => return Ok(()),
            _ => return Err(PromisqsError::WouldBlock),
        }
    }

    // Returns the number of items currently on the shared memory queue
    pub fn len(&self) -> usize {
        self.shmem.end - self.shmem.head
    }

    // Gets the total capacity of the queue
    pub fn capacity(&self) -> usize {
        self.shmem.capacity
    }

    // Returns whether or not there is any space left in the queue
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    // Checks if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Tries to push an element to the queue in a non-blocking manner
    /// meaning if the first attempt at acquiring the lock for the queue fails
    /// then this function will return a WouldBlock error.
    /// is returned instead of trying again. This might happen if two
    /// separate threads/processes are trying to push to the queue at the same time.
    ///
    /// If there is only one producer then this function will never fail.
    ///
    /// Since it is non-blocking, this function is a fixed time operation.
    pub fn try_push(&mut self, t: &T) -> PromisqsResult<()> {
        self.try_lock()?;
        unsafe {
            let end = self.shmem.end;
            let head = self.shmem.head;
            let len = end - head;
            if len == self.shmem.capacity {
                self.unlock().unwrap();
                return Err(PromisqsError::QueueFull);
            }
            let offset = end % self.capacity();
            let w_ptr = self.data_ptr.clone().add(offset) as *mut _ as *mut u8;
            t.as_bytes()
                .iter()
                .enumerate()
                .for_each(|(i, b)| *w_ptr.add(i) = *b);
            self.shmem.end += 1;
            self.unlock().unwrap();
            Ok(())
        }
    }

    /// Push a new element to the queue.
    ///
    /// This function is blocking, meaning if acquiring the lock fails
    /// it will simply retry until it succeeds.
    ///
    /// If there is only one thread/process that is pushing items
    /// then this function is a fixed time operation and will not block.
    pub fn push(&mut self, t: &T) -> PromisqsResult<()> {
        // If another process manages write in between bounds checking
        // and offset update we simply try again
        // This is also effectivley ABA safe since the write offset is only
        // incremented and never decremented
        loop {
            match self.try_push(t) {
                Ok(()) => return Ok(()),
                Err(PromisqsError::WouldBlock) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Tries to an pop and item from the queue in a non-blocking manner
    /// meaning if the first attempt at acquiring the lock fails, a WouldBlock error is returned.
    ///
    /// No other errors are possible for this function is possible, and if the queue is empty
    /// an Ok(None) is returned instead.
    ///
    /// If only one process/thread is reading from the queue then this function is infallible.
    pub fn try_pop(&mut self) -> PromisqsResult<Option<T>> {
        self.try_lock()?;
        unsafe {
            let head = self.shmem.head;
            let end = self.shmem.end;
            // Queue is empty
            if head == end {
                self.unlock().unwrap();
                return Ok(None);
            }
            let r_ptr = self.data_ptr.clone().add(head % self.shmem.capacity) as *mut _ as *mut u8;
            let s = std::slice::from_raw_parts(r_ptr, self.element_size as usize);
            let t = FromBytes::read_from(s).unwrap();
            self.shmem.head += 1;
            self.unlock().unwrap();
            Ok(Some(t))
        }
    }

    /// Pop an element from the queue.
    ///
    /// This function is blocking, meaning if acquiring the lock fails
    /// it will simply retry until it succeeds.
    ///
    /// If there is only one thread/process that is reading items
    /// then this function is a fixed time operation and will not block.
    pub fn pop(&mut self) -> Option<T> {
        // If another process manages write in between bounds checking
        // and offset update we simply try again
        // This is also effectivley ABA safe since the write offset is only
        // incremented and never decremented
        loop {
            match self.try_pop() {
                Ok(v) => return v,
                // try_pop can only return WouldBlock errors so we can safely ignore all other
                // error variants and do not have to return a result here
                Err(_) => continue,
            }
        }
    }

    /// Peeks the item that is next in the queue without popping it
    pub fn try_peek(&self) -> PromisqsResult<Option<T>> {
        self.try_lock()?;
        unsafe {
            let head = self.shmem.head;
            let end = self.shmem.end;
            // Queue is empty
            if head == end {
                self.unlock().unwrap();
                return Ok(None);
            }
            let r_ptr = self.data_ptr.clone().add(head % self.shmem.capacity) as *mut _ as *mut u8;
            let s = std::slice::from_raw_parts(r_ptr, self.element_size as usize);
            let t = FromBytes::read_from(s).unwrap();
            self.unlock().unwrap();
            Ok(Some(t))
        }
    }

    /// Peeks the item that is next in the queue without popping it in a blocking manner.
    ///
    /// If there is only one process/thread reading from the queue, then this function
    /// is fixed time and will never fail.
    pub fn peek(&self) -> Option<T> {
        loop {
            match self.try_peek() {
                Ok(v) => return v,
                // try_pop can only return WouldBlock errors so we can safely ignore all other
                // error variants and do not have to return a result here
                Err(_) => continue,
            }
        }
    }
}

impl<'q, T: AsBytes + FromBytes> Iterator for ShmemQueue<'q, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.pop()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{DateTime, Local};
    use crossbeam_channel::{Receiver, Sender};
    use rand::distributions::{Alphanumeric, DistString};
    use std::path::Path;
    use std::sync::{atomic::AtomicI64, Arc, Barrier};
    use std::time::Duration;

    // Generates a random 16-byte alphanumeric string map name
    fn gen_map_name() -> String {
        Alphanumeric.sample_string(&mut rand::thread_rng(), 16) + ".map"
    }

    #[test]
    fn test_create_drop_queue() {
        let q_file_name = gen_map_name();
        let q: ShmemQueue<u32> = ShmemQueue::create(&q_file_name, 10).unwrap();
        let q_file = Path::new(&q_file_name);
        assert!(q_file.exists());
        assert!(q.len() == 0);
        assert_eq!(q.capacity(), 10);
        assert_eq!(q.element_size, 4);
        assert_eq!(q.shmem.ref_cnt.load(Ordering::SeqCst), 1);

        let q_n: ShmemQueue<u32> = ShmemQueue::open(&q_file_name).unwrap();
        assert!(q_file.exists());
        assert!(q.len() == 0);
        assert_eq!(q_n.capacity(), 10);
        assert_eq!(q_n.element_size, 4);

        // Check that the reference count is correct
        assert_eq!(q_n.shmem.ref_cnt.load(Ordering::SeqCst), 2);
        assert_eq!(q.shmem.ref_cnt.load(Ordering::SeqCst), 2);

        // Drop one of the queues and check that the ref count is decremented
        drop(q_n);
        assert!(q_file.exists());

        assert_eq!(q.shmem.ref_cnt.load(Ordering::SeqCst), 1);
        drop(q);
        std::thread::sleep(Duration::from_millis(1));
        assert!(!q_file.exists());
    }

    #[test]
    fn test_push_and_pop() {
        let mut q: ShmemQueue<u32> = ShmemQueue::create(&gen_map_name(), 10).unwrap();
        assert!(q.push(&69).is_ok());
        assert_eq!(q.shmem.end, 1);
        assert_eq!(q.shmem.head, 0);
        assert_eq!(q.pop(), Some(69));
        assert_eq!(q.shmem.head, 1);
    }

    #[test]
    fn test_pop_emtpy() {
        let mut q: ShmemQueue<u32> = ShmemQueue::create(&gen_map_name(), 10).unwrap();
        assert!(q.pop().is_none());

        // Push an element and pop it and then try to pop the empty queue again.
        assert!(q.push(&69).is_ok());
        assert_eq!(q.pop(), Some(69));
        assert!(q.pop().is_none());
    }

    #[test]
    fn test_push_full() {
        let mut q: ShmemQueue<u32> = ShmemQueue::create(&gen_map_name(), 10).unwrap();
        for item in (0..10).into_iter() {
            assert!(q.push(&item).is_ok());
        }
        assert!(q.push(&69).is_err());
    }

    // Test that the values are read back correctly when the write and read
    // pointer wrap around to the start of the buffer
    #[test]
    fn test_buffer_wrapping() {
        let capacity = 10;
        let mut q = ShmemQueue::create(&gen_map_name(), capacity).unwrap();
        let v1 = [255_u8; 1024];
        for _ in 0..capacity {
            q.push(&v1).unwrap();
        }
        assert!(q.pop().is_some());
        let v2 = [133_u8; 1024];
        assert!(q.push(&v2).is_ok());
        for _ in 0..(capacity - 1) {
            assert_eq!(v1, q.pop().unwrap());
        }
        assert_eq!(v2, q.pop().unwrap());
    }

    // Test that the values are read back correctly when the write and read
    // pointer wrap around to the start of the buffer
    #[test]
    fn test_peek() {
        let capacity = 10;
        let mut q = ShmemQueue::create(&gen_map_name(), capacity).unwrap();
        let v1 = [255_u8; 1024];
        assert!(q.peek().is_none());
        q.push(&v1).unwrap();
        assert_eq!(q.shmem.head, 0);
        assert_eq!(q.peek(), Some(v1));
        assert_eq!(q.shmem.head, 0);
        assert_eq!(q.peek(), q.pop());
    }

    #[test]
    fn test_smoke_threaded_writes() {
        const N_THREADS: usize = 10;
        const N_REPETITIONS: usize = 10000;

        let q_file_name = gen_map_name();
        let mut q: ShmemQueue<u64> = ShmemQueue::create(&q_file_name, N_THREADS).unwrap();

        for _ in 0..N_REPETITIONS {
            let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(N_THREADS);
            let barrier = Arc::new(Barrier::new(N_THREADS));

            for i in (0..N_THREADS).into_iter() {
                let q_file_name_clone = q_file_name.clone();
                let barrier_clone = barrier.clone();
                let h = std::thread::spawn(move || {
                    let mut q_prime: ShmemQueue<u64> =
                        ShmemQueue::open(&q_file_name_clone).unwrap();
                    barrier_clone.wait();
                    assert!(q_prime.push(&(i as u64)).is_ok());
                    // Ignore result, only the fastest thread will succeed
                });
                handles.push(h);
            }

            for handle in handles {
                handle.join().unwrap();
            }
            let vals: Vec<u64> = (0..N_THREADS)
                .into_iter()
                .map(|_| q.pop().unwrap())
                .collect();

            (0..N_THREADS)
                .into_iter()
                .for_each(|i| assert!(vals.contains(&(i as u64))));

            assert!(q.is_empty());
        }
    }

    // Conducts a series of simultaneous reads and writes and ensures
    // that no data corruption happens
    #[test]
    fn test_smoke_simultaneous_read_write() {
        const N_REPETITIONS: usize = 10000;

        let q_file_name = gen_map_name();
        let mut q: ShmemQueue<u64> = ShmemQueue::create(&q_file_name, 2).unwrap();

        for _ in 0..N_REPETITIONS {
            let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(2);
            let barrier = Arc::new(Barrier::new(2));

            // Push initial element so that the read does not just return None
            q.push(&1).unwrap();

            // Reading thread
            let q_file_name_clone = q_file_name.clone();
            let barrier_clone = barrier.clone();
            let h = std::thread::spawn(move || {
                let mut q_prime: ShmemQueue<u64> = ShmemQueue::open(&q_file_name_clone).unwrap();
                barrier_clone.wait();
                assert_eq!(q_prime.pop(), Some(1));
                // Ignore result, only the fastest thread will succeed
            });
            handles.push(h);

            // Writing thread
            let q_file_name_clone = q_file_name.clone();
            let barrier_clone = barrier.clone();
            let h = std::thread::spawn(move || {
                let mut q_prime: ShmemQueue<u64> = ShmemQueue::open(&q_file_name_clone).unwrap();
                barrier_clone.wait();
                assert!(q_prime.push(&2).is_ok());
                // Ignore result, only the fastest thread will succeed
            });
            handles.push(h);

            for handle in handles {
                handle.join().unwrap();
            }
            assert_eq!(q.pop(), Some(2));
            assert!(q.is_empty());
        }
    }
}
