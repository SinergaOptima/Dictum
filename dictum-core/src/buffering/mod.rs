//! Lock-free SPSC ring buffer for audio samples.
//!
//! Uses `ringbuf::HeapRb<f32>` which provides a wait-free `push_slice`
//! safe to call from the real-time audio callback.

pub mod chunk;

use ringbuf::{traits::Split, HeapRb};

pub use ringbuf::traits::{Consumer, Producer};

/// Type alias for the producer half — held by the audio callback thread.
pub type AudioProducer = ringbuf::HeapProd<f32>;

/// Type alias for the consumer half — held by the pipeline thread.
pub type AudioConsumer = ringbuf::HeapCons<f32>;

/// Buffer capacity: 2^22 = 4 194 304 f32 samples ≈ 87.4 s at 48 kHz.
/// This protects long dictation from callback drops while final inference runs.
pub const RING_CAPACITY: usize = 1 << 22;

/// Create a matched producer/consumer pair backed by a heap-allocated ring buffer.
///
/// # Panics
/// Never panics — `HeapRb` construction cannot fail for reasonable capacities.
pub fn create_audio_ring() -> (AudioProducer, AudioConsumer) {
    HeapRb::<f32>::new(RING_CAPACITY).split()
}
