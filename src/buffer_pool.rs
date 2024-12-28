use bytes::BytesMut;
use log::debug;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::Mutex as AsyncMutex;

pub const MAX_BUF_SIZE: usize = 8192;
pub const POOL_SIZE: usize = 200;

// Sharded Buffer Pool for high concurrency
#[derive(Clone)]
pub struct ShardedBufferPool {
    shards: Vec<Arc<BufferPoolShard>>,
}

impl ShardedBufferPool {
    pub fn new(num_shards: usize, pool_size: usize) -> Self {
        debug!(
            "Initializing ShardedBufferPool with {} shards and {} pool size each",
            num_shards, pool_size
        );
        let shards = (0..num_shards)
            .map(|_| Arc::new(BufferPoolShard::new(pool_size)))
            .collect();
        Self { shards }
    }

    pub async fn get_buffer(&self, id: usize) -> BytesMut {
        let shard_index = id % self.shards.len();
        debug!("Requesting buffer from shard {}", shard_index);
        let shard = &self.shards[shard_index];
        shard.get_buffer().await
    }

    pub async fn return_buffer(&self, id: usize, buffer: BytesMut) {
        let shard_index = id % self.shards.len();
        debug!("Returning buffer to shard {}", shard_index);
        let shard = &self.shards[shard_index];
        shard.return_buffer(buffer).await;
    }
}

struct BufferPoolShard {
    buffers: AsyncMutex<VecDeque<BytesMut>>,
}

impl BufferPoolShard {
    fn new(pool_size: usize) -> Self {
        debug!("Creating new BufferPoolShard with pool size {}", pool_size);
        let mut buffers = VecDeque::with_capacity(pool_size);
        for _ in 0..pool_size {
            buffers.push_back(BytesMut::with_capacity(MAX_BUF_SIZE));
        }
        Self {
            buffers: AsyncMutex::new(buffers),
        }
    }

    async fn get_buffer(&self) -> BytesMut {
        let mut buffers = self.buffers.lock().await;
        if let Some(mut buffer) = buffers.pop_front() {
            debug!(
                "Acquired buffer from pool, buffers remaining: {}",
                buffers.len()
            );
            if buffer.capacity() < MAX_BUF_SIZE {
                debug!("Buffer capacity is less than max, reallocating");
                buffer = BytesMut::with_capacity(MAX_BUF_SIZE);
            } else {
                debug!("Clearing buffer for reuse");
                buffer.clear(); // Prepare buffer for reuse
            }
            buffer
        } else {
            debug!("No buffer available in pool, allocating new buffer");
            BytesMut::with_capacity(MAX_BUF_SIZE)
        }
    }

    async fn return_buffer(&self, buffer: BytesMut) {
        let mut buffers = self.buffers.lock().await;
        if buffers.len() < POOL_SIZE {
            debug!(
                "Returning buffer to pool, buffers now: {}",
                buffers.len() + 1
            );
            buffers.push_back(buffer);
        } else {
            debug!("Buffer pool full, discarding buffer");
        }
    }
}
