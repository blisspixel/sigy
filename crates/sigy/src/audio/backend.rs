use super::{
    Failure,
    pcm::Queue,
    protocol::{self, Event, HelperLimits, Ready, Report, Status},
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    io::{Read, Write},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

struct Output {
    stream: cpal::Stream,
    queue: Arc<Queue>,
    ready: Ready,
    anchor_ns: u64,
}

pub(super) fn run(
    input: &mut impl Read,
    stdout: &mut impl Write,
    limits: HelperLimits,
) -> Result<(), Failure> {
    let output = match open(limits) {
        Ok(output) => output,
        Err(error) => {
            protocol::write_event(stdout, &super::helper::startup_failure(error.as_ref()))?;
            return Err(error);
        }
    };
    protocol::write_event(stdout, &Event::Ready(output.ready.clone()))?;
    let outcome = feed(input, &output, limits).and_then(|()| drain(&output));
    // The parent sees this final message only after CPAL's native owner has joined.
    // If Drop blocks, the parent's process deadline remains the containment boundary.
    let Output {
        stream,
        queue,
        ready: _,
        anchor_ns,
    } = output;
    drop(stream);
    let outcome = if queue.failed.load(Ordering::Acquire) {
        Err("audio-device-failed".into())
    } else {
        outcome
    };
    let error = outcome
        .as_ref()
        .err()
        .map(|error| super::helper::failure_code(error.as_ref(), false));
    let report = report(&queue, anchor_ns, error);
    protocol::write_event(stdout, &Event::Report(report))?;
    outcome
}

fn open(limits: HelperLimits) -> Result<Output, Failure> {
    let host = cpal::default_host();
    let default = host
        .default_output_device()
        .ok_or("audio-device-unavailable")?;
    // Freeze the current endpoint instead of opening the virtual default-device handle.
    let id = default.id().map_err(|_| "audio-device-identity")?;
    let device = host.device_by_id(&id).ok_or("audio-device-unavailable")?;
    let supported = device
        .default_output_config()
        .map_err(|_| "audio-device-config")?;
    let rate = supported.sample_rate();
    let channels = supported.channels();
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err("audio-sample-format-unsupported".into());
    }
    let maximum_pcm_bytes = super::pcm::maximum_bytes(limits.duration_us, rate, channels)?;
    let queue = Arc::new(Queue::new(rate, channels)?);
    let mut config = supported.config();
    config.buffer_size = cpal::BufferSize::Fixed((rate / 100).max(1));
    let callback = Arc::clone(&queue);
    let errors = Arc::clone(&queue);
    let stream = device
        .build_output_stream::<f32, _, _>(
            config,
            move |samples, info| render(&callback, samples, info, rate),
            move |_| {
                errors.failed.store(true, Ordering::Release);
            },
            Some(Duration::from_millis(protocol::STARTUP_MS)),
        )
        .map_err(|_| "audio-device-open")?;
    let anchor_ns = u64::try_from(stream.now().as_nanos())?;
    let ready = Ready {
        protocol: 1,
        rate_hz: rate,
        channels,
        sample_format: "f32le".into(),
        ring_frames: u32::try_from(queue.capacity())?,
        callback_frames_estimate: stream.buffer_size().map_err(|_| "audio-buffer-estimate")?,
        maximum_pcm_bytes,
    };
    ready.validate(limits)?;
    Ok(Output {
        stream,
        queue,
        ready,
        anchor_ns,
    })
}

fn render(queue: &Queue, samples: &mut [f32], info: &cpal::OutputCallbackInfo, rate: u32) {
    if queue.failed.load(Ordering::Acquire) {
        samples.fill(0.0);
        return;
    }
    if let Some(last) = queue.render(samples) {
        let target = u64::try_from(info.timestamp().playback.as_nanos())
            .ok()
            .zip(u64::try_from(last).ok())
            .and_then(|(start, frames)| {
                frames
                    .checked_mul(1_000_000_000)
                    .and_then(|ns| ns.checked_add(u64::from(rate) - 1))
                    .and_then(|ns| start.checked_add(ns / u64::from(rate)))
            });
        if let Some(target) = target {
            queue.publish_presentation(target);
        } else {
            queue.failed.store(true, Ordering::Release);
        }
    }
}

fn feed(input: &mut impl Read, output: &Output, limits: HelperLimits) -> Result<(), Failure> {
    let deadline = Instant::now()
        .checked_add(
            Duration::from_micros(limits.duration_us) + Duration::from_millis(protocol::STARTUP_MS),
        )
        .ok_or("audio-deadline-overflow")?;
    let mut bytes = vec![0; protocol::PCM_BYTES].into_boxed_slice();
    let mut total = 0_u64;
    let channels = usize::from(output.ready.channels);
    let preroll = super::pcm::preroll_frames(limits.duration_us, output.ready.rate_hz)?;
    let mut started = false;
    loop {
        let length = super::pcm::read_frame(
            input,
            &mut bytes,
            output.ready.channels,
            &mut total,
            output.ready.maximum_pcm_bytes,
        )?;
        check(output, deadline)?;
        if length == 0 {
            output.queue.ended.store(true, Ordering::Release);
            if !started {
                output.stream.play().map_err(|_| "audio-device-start")?;
            }
            return Ok(());
        }
        for frame in bytes[..length].chunks_exact(channels * 4) {
            let mut samples = [0.0; 2];
            for (sample, value) in samples[..channels].iter_mut().zip(frame.as_chunks::<4>().0) {
                *sample = f32::from_le_bytes(*value);
            }
            while !output.queue.push(&samples[..channels])? {
                check(output, deadline)?;
                std::thread::sleep(Duration::from_millis(1));
            }
            if !started && output.queue.decoded() >= preroll {
                output.stream.play().map_err(|_| "audio-device-start")?;
                started = true;
            }
            check(output, deadline)?;
        }
    }
}

fn check(output: &Output, deadline: Instant) -> Result<(), Failure> {
    super::helper::check_work(
        output.queue.failed.load(Ordering::Acquire),
        deadline,
        Instant::now(),
    )
}

fn drain(output: &Output) -> Result<(), Failure> {
    let deadline = Instant::now() + Duration::from_millis(protocol::DRAIN_MS);
    loop {
        check(output, deadline)?;
        if output.queue.ready_to_drain(output.stream.now().as_nanos()) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn report(queue: &Queue, anchor_ns: u64, error: Option<String>) -> Report {
    Report {
        protocol: 1,
        status: if error.is_none() {
            Status::Drained
        } else {
            Status::Failed
        },
        error,
        decoded_frames: queue.decoded(),
        content_frames: queue.content(),
        clipped_samples: queue.clipped_samples.load(Ordering::Acquire),
        underrun_frames: queue.underrun.load(Ordering::Relaxed),
        drain_zero_frames: queue.drain_zeros.load(Ordering::Relaxed),
        callbacks: queue.callbacks.load(Ordering::Relaxed),
        queue_high_water_frames: queue.high_water.load(Ordering::Relaxed),
        predicted_presentation_us: queue
            .presentation_ns
            .load(Ordering::Acquire)
            .checked_sub(anchor_ns)
            .map(|ns| ns / 1000),
        presentation_is_estimated: true,
        audibility_proven: false,
    }
}
