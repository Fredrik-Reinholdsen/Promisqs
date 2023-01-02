use crate::error::PromisqsError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use shared_memory as shmem;
use zerocopy::{AsBytes, FromBytes};

pub type PromisqsResult<T> = Result<T, PromisqsError>;

// Struct that details the layout of the head of the shared memory buffer
// NOTE Not meant to be instatiatied directly.
#[repr(C, align(8))]
#[derive(Debug)]
pub(crate) struct SharedMemory {
    // Flag that signifies if the shared memory queue has been initialized
    is_init: AtomicBool,
    lock: AtomicBool,
    ref_cnt: AtomicU64,
    // Serialized size of each queue element in bytes
    element_size: AtomicU64,
    capacity: AtomicU64,
    read_offset: AtomicU64,
    write_offset: AtomicU64,
}

// The memory of this struct is owned by whoever owns the mmap
// and so is handled by the drop functions for the mmap
// this struct is really only meant as an interface instead of using pointers
impl Drop for SharedMemory {
    fn drop(&mut self) {
        std::mem::forget(self)
    }
}

pub struct ShmemQueue<'q, T: AsBytes + FromBytes> {
    // Marks the type of the queue
    _marker: std::marker::PhantomData<T>,
    // Memmap shared memory object
    mmap: shmem::Shmem,
    // Size of the shared memory portion used for the queue
    data_size: u64,
    // A reference to the header data in shared memory
    shmem: &'q SharedMemory,
    // Pointer to the base of the queue
    data_ptr: *mut u8,
    // The size of each element of the queue in bytes
    element_size: u64,
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
        self.shmem.ref_cnt.fetch_add(1, Ordering::Release);
        Self {
            mmap: mmap_clone,
            shmem: self.shmem.clone(),
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
        let element_size = unsafe { (*(&buf as *const _ as *const T)).as_bytes().len() } as u64;
        let data_size = element_size * capacity as u64;
        let head_size = std::mem::size_of::<SharedMemory>() as u64;
        let mmap = shmem::ShmemConf::new()
            .size((data_size + head_size) as usize)
            .flink(flink)
            .create()?;
        let ptr = mmap.as_ptr();
        // Initialize the shared memory atomics
        let shmem = unsafe { &*(ptr as *const _ as *const SharedMemory) };
        shmem.ref_cnt.store(1, Ordering::Relaxed);
        shmem.element_size.store(element_size, Ordering::Relaxed);
        shmem.lock.store(false, Ordering::Relaxed);
        shmem.capacity.store(capacity as u64, Ordering::Relaxed);
        shmem.read_offset.store(0, Ordering::Relaxed);
        shmem.write_offset.store(0, Ordering::Relaxed);
        shmem.is_init.store(true, Ordering::Release);

        let data_ptr = unsafe { ptr.add(head_size as usize) };

        let s = Self {
            _marker: std::marker::PhantomData,
            data_size,
            mmap,
            data_ptr,
            shmem,
            element_size: element_size as u64,
        };
        // Mark the shared memory as initialized
        Ok(s)
    }
    // Open and link to an already created shared memory queue
    pub fn open(flink: &str) -> PromisqsResult<Self> {
        let mmap = shmem::ShmemConf::new().flink(flink).open()?;
        let ptr = mmap.as_ptr();
        let shmem = unsafe { &*(ptr as *const _ as *const SharedMemory) };
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
        let element_size = unsafe { (*(&buf as *const _ as *const T)).as_bytes().len() } as u64;
        if element_size != shmem.element_size.load(Ordering::Relaxed) {
            return Err(PromisqsError::ElementSizeMismatch);
        }

        let data_size = element_size as usize * shmem.capacity.load(Ordering::Relaxed) as usize;
        let head_size = std::mem::size_of::<SharedMemory>();
        let data_ptr = unsafe { ptr.add(head_size) };

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

    // Gets the current length of the queue
    pub fn len(&self) -> usize {
        let read_offset = self.shmem.read_offset.load(Ordering::SeqCst);
        let write_offset = self.shmem.write_offset.load(Ordering::SeqCst);
        (write_offset.overflowing_sub(read_offset).0 / self.element_size) as usize
    }

    // Gets the total capacity of the queue
    pub fn capacity(&self) -> usize {
        self.shmem.capacity.load(Ordering::Relaxed) as usize
    }

    // Checks if there is space left in the queue
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    // Checks if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // Push a new element to the queue
    pub fn push(&mut self, t: &T) -> PromisqsResult<()> {
        unsafe {
            // If another process manages write in between bounds checking
            // and offset update we simply try again
            // This is also effectivley ABA safe since the write offset is only
            // incremented and never decremented
            let write_offset = loop {
                // Load the write_offset first because it is checked later, so even if
                // the read_offset changes between the load of write_offset and the load
                // of read_offset the length is still correct
                let write_offset = self.shmem.write_offset.load(Ordering::SeqCst);
                let read_offset = self.shmem.read_offset.load(Ordering::SeqCst);
                let len = (write_offset - read_offset) / self.element_size;
                if len >= self.capacity() as u64 {
                    return Err(PromisqsError::QueueFull);
                }
                // Make sure that the length has not changed since
                // bounds check. If not increment the len to make sure
                // that no other thread writes to this location in memory
                match self.shmem.write_offset.compare_exchange_weak(
                    write_offset,
                    write_offset + self.element_size,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                ) {
                    Ok(offset) => break offset,
                    Err(_) => continue,
                }
            };
            let offset = (write_offset % self.data_size) as usize;
            let w_ptr = self.data_ptr.add(offset);
            t.as_bytes()
                .iter()
                .enumerate()
                .for_each(|(i, b)| *w_ptr.add(i) = *b);
            Ok(())
        }
    }

    // Pop and return the first element from the queue
    // Popping the element queue means it can no longer be read
    // and the memory location is free to be overwritten
    pub fn pop(&mut self) -> PromisqsResult<T> {
        unsafe {
            // If another process manages write in between bounds checking
            // and offset update we simply try again
            // This is also effectivley ABA safe since the write offset is only
            // incremented and never decremented
            let read_offset = loop {
                let read_offset = self.shmem.read_offset.load(Ordering::SeqCst);
                let write_offset = self.shmem.write_offset.load(Ordering::SeqCst);
                let len = (write_offset - read_offset) / self.element_size;
                if len == 0 {
                    return Err(PromisqsError::QueueEmpty);
                }
                // Make sure that the length has not changed since
                // bounds check. If not increment the len to make sure
                // that no other thread writes to this location in memory
                match self.shmem.read_offset.compare_exchange_weak(
                    read_offset,
                    read_offset + self.element_size,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                ) {
                    Ok(offset) => break offset,
                    Err(_) => continue,
                }
            };
            let offset = (read_offset % self.data_size) as usize;
            let r_ptr = self.data_ptr.add(offset);
            let s = std::slice::from_raw_parts(r_ptr, self.element_size as usize);
            let t = FromBytes::read_from(s).unwrap();
            Ok(t)
        }
    }

    // Reads the first element of the queue without moving
    // the read pointer/deallocating it
    pub fn peek(&self) -> PromisqsResult<T> {
        unsafe {
            let read_offset = self.shmem.read_offset.load(Ordering::SeqCst);
            let write_offset = self.shmem.write_offset.load(Ordering::SeqCst);
            let len = (write_offset - read_offset) / self.element_size;
            if len == 0 {
                return Err(PromisqsError::QueueEmpty);
            }
            let offset = (read_offset % self.data_size) as usize;
            let r_ptr = self.data_ptr.add(offset);
            let s = std::slice::from_raw_parts(r_ptr, self.element_size as usize);
            let t = FromBytes::read_from(s).unwrap();
            Ok(t)
        }
    }
}

impl<'q, T: AsBytes + FromBytes> Iterator for ShmemQueue<'q, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        match self.pop() {
            Ok(t) => Some(t),
            Err(_) => None,
        }
    }
}
