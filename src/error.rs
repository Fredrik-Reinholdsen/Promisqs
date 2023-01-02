use shared_memory::ShmemError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PromisqsError {
    #[error(transparent)]
    ShmemError(#[from] ShmemError),
    #[error("Failed to open, queue is not initialized!")]
    BufferNotInitialized,
    #[error("Element size of queue in flink does not match T!")]
    ElementSizeMismatch,
    #[error("Push failed, queue is full!")]
    QueueFull,
    #[error("Queue is empty!")]
    QueueEmpty,
}
