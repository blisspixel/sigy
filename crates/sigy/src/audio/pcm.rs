use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub(crate) fn read_frame(
    input: &mut impl std::io::Read,
    bytes: &mut [u8],
    channels: u16,
    total: &mut u64,
    maximum: u64,
) -> Result<usize, super::Failure> {
    if !(1..=2).contains(&channels) {
        return Err("audio-channels-unsupported".into());
    }
    let length = usize::try_from(super::protocol::read_length(input)?)?;
    if length == 0 {
        let mut trailing = [0];
        if input.read(&mut trailing)? != 0 {
            return Err("audio-trailing-input".into());
        }
        if *total == 0 {
            return Err("audio-empty-input".into());
        }
        return Ok(0);
    }
    if length > super::protocol::PCM_BYTES
        || length > bytes.len()
        || !length.is_multiple_of(usize::from(channels) * 4)
    {
        return Err("audio-frame-bound".into());
    }
    let next = total
        .checked_add(u64::try_from(length)?)
        .ok_or("audio-pcm-bound")?;
    if next > maximum {
        return Err("audio-pcm-bound".into());
    }
    input.read_exact(&mut bytes[..length])?;
    for sample in bytes[..length].as_chunks::<4>().0 {
        if !f32::from_le_bytes(*sample).is_finite() {
            return Err("audio-sample-invalid".into());
        }
    }
    *total = next;
    Ok(length)
}

pub(crate) fn maximum_bytes(
    duration_us: u64,
    rate: u32,
    channels: u16,
) -> Result<u64, super::Failure> {
    super::protocol::HelperLimits { duration_us }.validate()?;
    if !(8000..=192_000).contains(&rate) || !(1..=2).contains(&channels) {
        return Err("audio-config-unsupported".into());
    }
    duration_us
        .checked_add(100_000)
        .and_then(|duration| duration.checked_mul(u64::from(rate)))
        .and_then(|value| value.checked_add(999_999))
        .map(|value| value / 1_000_000)
        .and_then(|frames| frames.checked_mul(u64::from(channels) * 4))
        .ok_or_else(|| "audio-pcm-bound".into())
}

pub(crate) fn preroll_frames(duration_us: u64, rate: u32) -> Result<u64, super::Failure> {
    maximum_bytes(duration_us, rate, 1)?;
    let requested = duration_us
        .checked_mul(u64::from(rate))
        .and_then(|value| value.checked_add(999_999))
        .ok_or("audio-preroll-bound")?
        / 1_000_000;
    Ok(requested.min(u64::from(rate / 20)))
}

/// Exactly one producer and consumer. Frames publish only after all channel slots.
pub(crate) struct Queue {
    slots: Vec<AtomicU32>,
    channels: usize,
    capacity: u64,
    written: AtomicU64,
    consumed: AtomicU64,
    pub(crate) ended: AtomicBool,
    pub(crate) failed: AtomicBool,
    pub(crate) callbacks: AtomicU64,
    pub(crate) underrun: AtomicU64,
    pub(crate) drain_zeros: AtomicU64,
    pub(crate) high_water: AtomicU64,
    pub(crate) clipped_samples: AtomicU64,
    pub(crate) presentation_ns: AtomicU64,
    scheduled: AtomicU64,
}

impl Queue {
    pub(crate) fn new(rate: u32, channels: u16) -> Result<Self, super::Failure> {
        maximum_bytes(1, rate, channels)?;
        let capacity = u64::from(rate / 4);
        let samples = usize::try_from(capacity)?
            .checked_mul(usize::from(channels))
            .ok_or("audio-ring-bound")?;
        if samples.checked_mul(4).is_none_or(|bytes| bytes > 1_048_576) {
            return Err("audio-ring-bound".into());
        }
        Ok(Self {
            slots: (0..samples).map(|_| AtomicU32::new(0)).collect(),
            channels: usize::from(channels),
            capacity,
            written: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            underrun: AtomicU64::new(0),
            drain_zeros: AtomicU64::new(0),
            high_water: AtomicU64::new(0),
            clipped_samples: AtomicU64::new(0),
            presentation_ns: AtomicU64::new(0),
            scheduled: AtomicU64::new(0),
        })
    }

    pub(crate) fn capacity(&self) -> u64 {
        self.capacity
    }
    pub(crate) fn decoded(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }
    pub(crate) fn content(&self) -> u64 {
        self.consumed.load(Ordering::Acquire)
    }

    pub(crate) fn publish_presentation(&self, target_ns: u64) {
        self.presentation_ns.fetch_max(target_ns, Ordering::Relaxed);
        self.scheduled.store(self.content(), Ordering::Release);
    }

    pub(crate) fn ready_to_drain(&self, now_ns: u128) -> bool {
        let decoded = self.decoded();
        self.ended.load(Ordering::Acquire)
            && decoded != 0
            && self.scheduled.load(Ordering::Acquire) == decoded
            && self.content() == decoded
            && self.presentation_ns.load(Ordering::Acquire) != 0
            && now_ns >= u128::from(self.presentation_ns.load(Ordering::Acquire))
    }

    pub(crate) fn push(&self, samples: &[f32]) -> Result<bool, super::Failure> {
        if samples.len() != self.channels || samples.iter().any(|sample| !sample.is_finite()) {
            return Err("audio-sample-invalid".into());
        }
        let written = self.written.load(Ordering::Relaxed);
        let consumed = self.consumed.load(Ordering::Acquire);
        if written
            .checked_sub(consumed)
            .is_none_or(|count| count >= self.capacity)
        {
            return Ok(false);
        }
        let next = written.checked_add(1).ok_or("audio-frame-overflow")?;
        let clipped = u64::try_from(
            samples
                .iter()
                .filter(|sample| **sample < -1.0 || **sample > 1.0)
                .count(),
        )?;
        let next_clipped = self
            .clipped_samples
            .load(Ordering::Relaxed)
            .checked_add(clipped)
            .ok_or("audio-frame-overflow")?;
        let offset = usize::try_from(written % self.capacity)? * self.channels;
        for (slot, sample) in self.slots[offset..offset + self.channels]
            .iter()
            .zip(samples)
        {
            // Only device output is saturated. Framed input bytes remain unchanged.
            slot.store(sample.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
        }
        self.clipped_samples.store(next_clipped, Ordering::Relaxed);
        self.high_water
            .fetch_max(next - consumed, Ordering::Relaxed);
        self.written.store(next, Ordering::Release);
        Ok(true)
    }

    /// Returns the one-based offset of the last content frame in this callback.
    pub(crate) fn render(&self, output: &mut [f32]) -> Option<usize> {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        if !output.len().is_multiple_of(self.channels)
            || u64::try_from(output.len() / self.channels)
                .ok()
                .is_none_or(|frames| frames > self.capacity)
        {
            output.fill(0.0);
            self.failed.store(true, Ordering::Release);
            return None;
        }
        let mut last = None;
        for (index, frame) in output.chunks_exact_mut(self.channels).enumerate() {
            let consumed = self.consumed.load(Ordering::Relaxed);
            if consumed < self.written.load(Ordering::Acquire) {
                let Ok(offset) = usize::try_from(consumed % self.capacity) else {
                    self.failed.store(true, Ordering::Release);
                    output.fill(0.0);
                    return None;
                };
                for (sample, slot) in frame.iter_mut().zip(&self.slots[offset * self.channels..]) {
                    *sample = f32::from_bits(slot.load(Ordering::Relaxed));
                }
                self.consumed.store(consumed + 1, Ordering::Release);
                last = Some(index + 1);
            } else {
                frame.fill(0.0);
                if self.ended.load(Ordering::Acquire) {
                    self.drain_zeros.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.underrun.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        last
    }
}
