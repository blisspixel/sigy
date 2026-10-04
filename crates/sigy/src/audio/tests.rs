use super::{
    pcm::{self, Queue},
    protocol::{self, Event, HelperLimits},
};
use std::{
    io::{Cursor, Read},
    sync::atomic::Ordering,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn independent_pcm_byte_and_profile_boundaries() -> TestResult {
    // One stereo second is 48,000 frames, plus the declared 4,800-frame tolerance.
    assert_eq!(pcm::maximum_bytes(1_000_000, 48_000, 2)?, 422_400);
    assert_eq!(pcm::maximum_bytes(1, 8000, 1)?, 3204);
    assert_eq!(
        pcm::maximum_bytes(1_800_000_000, 192_000, 2)?,
        2_764_953_600
    );
    for (duration, rate, channels) in [
        (0, 48000, 2),
        (u64::MAX, 48000, 2),
        (1_800_000_001, 48000, 2),
        (1, 7999, 1),
        (1, 192_001, 1),
        (1, 48000, 0),
        (1, 48000, 3),
    ] {
        assert!(pcm::maximum_bytes(duration, rate, channels).is_err());
    }
    Ok(())
}

#[test]
fn initial_preroll_uses_fifty_ms_or_the_entire_short_requested_interval() -> TestResult {
    assert_eq!(pcm::preroll_frames(1_000_000, 48_000)?, 2400);
    assert_eq!(pcm::preroll_frames(50_000, 48_000)?, 2400);
    assert_eq!(pcm::preroll_frames(25_000, 48_000)?, 1200);
    assert_eq!(pcm::preroll_frames(1, 8000)?, 1);
    assert_eq!(pcm::preroll_frames(1_800_000_000, 192_000)?, 9600);
    assert!(pcm::preroll_frames(u64::MAX, 48_000).is_err());
    Ok(())
}

fn framed(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut result = u32::try_from(bytes.len())?.to_le_bytes().to_vec();
    result.extend_from_slice(bytes);
    Ok(result)
}

#[test]
fn strict_limits_json_refuses_unknown_unbounded_and_truncated_inputs() -> TestResult {
    let mut legal = Cursor::new(framed(br#"{"duration_us":1000000}"#)?);
    assert_eq!(protocol::read_limits(&mut legal)?.duration_us, 1_000_000);
    for bytes in [
        br#"{"duration_us":1,"extra":1}"#.as_slice(),
        br#"{"duration_us":0}"#,
        br#"{"duration_us":-1}"#,
        br#"{"duration_us":18446744073709551616}"#,
        br#"{"duration_us":1,"duration_us":2}"#,
    ] {
        assert!(protocol::read_limits(&mut Cursor::new(framed(bytes)?)).is_err());
    }
    for bytes in [
        vec![0; 4],
        vec![1, 0],
        4097_u32.to_le_bytes().to_vec(),
        vec![2, 0, 0, 0, b'{'],
    ] {
        assert!(protocol::read_limits(&mut Cursor::new(bytes)).is_err());
    }
    let value = serde_json::to_vec(&HelperLimits { duration_us: 1 })?;
    assert!(value.len() < protocol::CONFIG_BYTES);
    assert!(
        serde_json::from_slice::<Event>(br#"{"event":"ready","protocol":1,"extra":0}"#).is_err()
    );
    Ok(())
}

#[test]
fn explicit_end_and_real_frame_limits_are_independent_of_read_boundaries() -> TestResult {
    let mut bytes = framed(&[0, 0, 0, 0, 0, 0, 128, 63])?;
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let mut input = TinyRead(Cursor::new(bytes));
    let mut output = vec![0; protocol::PCM_BYTES].into_boxed_slice();
    let mut total = 0;
    assert_eq!(
        pcm::read_frame(&mut input, &mut output, 2, &mut total, 8)?,
        8
    );
    assert_eq!(&output[..8], &[0, 0, 0, 0, 0, 0, 128, 63]);
    assert_eq!(
        pcm::read_frame(&mut input, &mut output, 2, &mut total, 8)?,
        0
    );
    for bytes in [
        vec![],
        vec![4, 0],
        vec![4, 0, 0, 0, 0],
        framed(&[0; 4])?,
        framed(&[0; 3])?,
        32769_u32.to_le_bytes().to_vec(),
    ] {
        assert!(pcm::read_frame(&mut Cursor::new(bytes), &mut output, 2, &mut 0, 32768).is_err());
    }
    assert!(
        pcm::read_frame(
            &mut Cursor::new(0_u32.to_le_bytes()),
            &mut output,
            1,
            &mut 0,
            8
        )
        .is_err()
    );
    let mut extra = vec![0; 4];
    extra.push(1);
    assert!(pcm::read_frame(&mut Cursor::new(extra), &mut output, 1, &mut 4, 8).is_err());
    let mut flood = Cursor::new(framed(&[0; 8])?);
    assert!(pcm::read_frame(&mut flood, &mut output, 2, &mut 8, 8).is_err());
    Ok(())
}

struct TinyRead(Cursor<Vec<u8>>);
impl Read for TinyRead {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let limit = bytes.len().min(1);
        self.0.read(&mut bytes[..limit])
    }
}

#[test]
fn finite_overshoot_bytes_survive_framing_but_nonfinite_input_never_commits() -> TestResult {
    let original: Vec<u8> = [1.25_f32, -2.0, 0.5, -0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let mut output = vec![0; protocol::PCM_BYTES].into_boxed_slice();
    let mut total = 0;
    assert_eq!(
        pcm::read_frame(
            &mut Cursor::new(framed(&original)?),
            &mut output,
            2,
            &mut total,
            16
        )?,
        16
    );
    assert_eq!(&output[..16], original);
    assert_eq!(total, 16);
    for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut refused_total = 0;
        assert!(
            pcm::read_frame(
                &mut Cursor::new(framed(&sample.to_le_bytes())?),
                &mut output,
                1,
                &mut refused_total,
                4
            )
            .is_err()
        );
        assert_eq!(refused_total, 0);
    }
    Ok(())
}

#[test]
fn output_saturation_preserves_signed_zero_and_counts_only_accepted_samples() -> TestResult {
    let queue = Queue::new(8000, 2)?;
    assert!(queue.push(&[1.25, -2.0])?);
    assert!(queue.push(&[0.5, -0.0])?);
    let mut output = [0.0_f32; 4];
    assert_eq!(queue.render(&mut output), Some(2));
    assert_eq!(
        output.map(f32::to_bits),
        [1.0_f32, -1.0, 0.5, -0.0].map(f32::to_bits)
    );
    assert_eq!(queue.clipped_samples.load(Ordering::Acquire), 2);
    // Every finite magnitude has the same explicit output policy.
    assert!(queue.push(&[f32::MAX, f32::MIN])?);
    let mut extremes = [0.0_f32; 2];
    assert_eq!(queue.render(&mut extremes), Some(1));
    assert_eq!(
        extremes.map(f32::to_bits),
        [1.0_f32, -1.0].map(f32::to_bits)
    );
    assert_eq!(queue.clipped_samples.load(Ordering::Acquire), 4);
    Ok(())
}

#[test]
fn repeated_full_queue_retries_cannot_inflate_clipping_receipt() -> TestResult {
    let queue = Queue::new(8000, 2)?;
    for _ in 0..2000 {
        assert!(queue.push(&[0.5, -0.5])?);
    }
    for _ in 0..8 {
        assert!(!queue.push(&[1.25, -2.0])?);
    }
    assert_eq!(queue.clipped_samples.load(Ordering::Acquire), 0);
    assert_eq!(queue.decoded(), 2000);
    let mut frame = [0.0; 2];
    assert_eq!(queue.render(&mut frame), Some(1));
    assert!(queue.push(&[1.25, -2.0])?);
    assert_eq!(queue.clipped_samples.load(Ordering::Acquire), 2);
    for _ in 0..8 {
        assert!(!queue.push(&[1.25, -2.0])?);
    }
    assert_eq!(queue.clipped_samples.load(Ordering::Acquire), 2);
    assert_eq!(queue.decoded(), 2001);
    let mut remaining = vec![0.0_f32; 4000];
    assert_eq!(queue.render(&mut remaining), Some(2000));
    assert_eq!(
        remaining[3998..]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        [1.0_f32.to_bits(), (-1.0_f32).to_bits()]
    );
    Ok(())
}

#[test]
fn stereo_content_survives_wrap_and_never_turns_shortage_into_source_silence() -> TestResult {
    let queue = Queue::new(8000, 2)?;
    assert_eq!(queue.capacity(), 2000);
    for _ in 0..2000 {
        assert!(queue.push(&[0.25, -0.75])?);
    }
    assert!(!queue.push(&[1.0, -1.0])?);
    let mut first = [0.0; 6];
    assert_eq!(queue.render(&mut first), Some(3));
    assert_eq!(
        first.map(f32::to_bits),
        [0.25_f32, -0.75, 0.25, -0.75, 0.25, -0.75].map(f32::to_bits)
    );
    for _ in 0..3 {
        assert!(queue.push(&[1.0, -1.0])?);
    }
    let mut remainder = vec![0.0; 4000];
    assert_eq!(queue.render(&mut remainder), Some(2000));
    assert_eq!(
        remainder[3994..]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        [1.0_f32, -1.0, 1.0, -1.0, 1.0, -1.0].map(f32::to_bits)
    );
    let mut empty = [1.0; 4];
    assert_eq!(queue.render(&mut empty), None);
    assert_eq!(empty.map(f32::to_bits), [0.0_f32; 4].map(f32::to_bits));
    assert_eq!(queue.underrun.load(Ordering::Relaxed), 2);
    queue.ended.store(true, Ordering::Release);
    queue.render(&mut empty);
    assert_eq!(queue.drain_zeros.load(Ordering::Relaxed), 2);
    assert_eq!(queue.content(), 2003);
    assert_eq!(queue.decoded(), 2003);
    assert_eq!(queue.high_water.load(Ordering::Relaxed), 2000);
    Ok(())
}

#[test]
fn malformed_samples_and_partial_channel_callbacks_fail_without_content() -> TestResult {
    let queue = Queue::new(8000, 2)?;
    for samples in [
        [f32::NAN, 0.0],
        [f32::INFINITY, 0.0],
        [f32::NEG_INFINITY, 0.0],
    ] {
        assert!(queue.push(&samples).is_err());
    }
    assert!(queue.push(&[1.0]).is_err());
    assert_eq!(queue.decoded(), 0);
    assert_eq!(queue.clipped_samples.load(Ordering::Relaxed), 0);
    let mut malformed = [1.0; 3];
    assert_eq!(queue.render(&mut malformed), None);
    assert_eq!(malformed.map(f32::to_bits), [0.0_f32; 3].map(f32::to_bits));
    assert!(queue.failed.load(Ordering::Acquire));
    assert_eq!(queue.content(), 0);
    Ok(())
}

#[test]
fn concurrent_stereo_sequence_matches_hand_declared_channel_oracle() -> TestResult {
    let queue = std::sync::Arc::new(Queue::new(8000, 2)?);
    let producer = std::sync::Arc::clone(&queue);
    let consumer = std::sync::Arc::clone(&queue);
    std::thread::scope(|scope| -> TestResult {
        let write = scope.spawn(move || -> Result<(), String> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let left = [-1.0, -0.5, 0.5, 1.0];
            for index in 0..10_000 {
                let value = left[index % 4];
                while !producer.push(&[value, -value]).map_err(|_| "producer")? {
                    if std::time::Instant::now() >= deadline {
                        return Err("producer deadline".into());
                    }
                    std::thread::yield_now();
                }
            }
            Ok(())
        });
        let read = scope.spawn(move || -> Result<(), String> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let expected = [-1.0_f32, -0.5, 0.5, 1.0];
            let mut frame = [0.0; 2];
            for index in 0..10_000 {
                while consumer.render(&mut frame).is_none() {
                    if std::time::Instant::now() >= deadline {
                        return Err("consumer deadline".into());
                    }
                    std::thread::yield_now();
                }
                if frame[0].to_bits() != expected[index % 4].to_bits()
                    || frame[1].to_bits() != (-expected[index % 4]).to_bits()
                {
                    return Err("stereo order/channel corruption".into());
                }
            }
            Ok(())
        });
        write
            .join()
            .map_err(|_| "producer panic")?
            .map_err(std::io::Error::other)?;
        read.join()
            .map_err(|_| "consumer panic")?
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    assert_eq!(queue.decoded(), 10_000);
    assert_eq!(queue.content(), 10_000);
    assert!(queue.high_water.load(Ordering::Relaxed) <= 2000);
    Ok(())
}

#[test]
fn handshake_validates_declared_profile_and_refuses_extra_fields() -> TestResult {
    let bytes = br#"{"event":"ready","protocol":1,"rate_hz":48000,"channels":2,"sample_format":"f32le","ring_frames":12000,"callback_frames_estimate":480,"maximum_pcm_bytes":422400}"#;
    let Event::Ready(mut ready) = serde_json::from_slice(bytes)? else {
        return Err("ready event".into());
    };
    ready.validate(HelperLimits {
        duration_us: 1_000_000,
    })?;
    ready.maximum_pcm_bytes += 1;
    assert!(
        ready
            .validate(HelperLimits {
                duration_us: 1_000_000
            })
            .is_err()
    );
    let mut unknown = bytes[..bytes.len() - 1].to_vec();
    unknown.extend_from_slice(b",\"extra\":1}");
    assert!(serde_json::from_slice::<Event>(&unknown).is_err());
    Ok(())
}

#[test]
fn consumed_frames_do_not_prove_the_last_callback_prediction_was_published() -> TestResult {
    let queue = Queue::new(8000, 1)?;
    let mut frame = [0.0];
    assert!(queue.push(&[0.5])?);
    assert_eq!(queue.render(&mut frame), Some(1));
    queue.publish_presentation(10);
    assert!(queue.push(&[-0.5])?);
    assert_eq!(queue.render(&mut frame), Some(1));
    queue.ended.store(true, Ordering::Release);
    // The old prediction elapsed, and all frames were consumed, but the second
    // callback has not published its later presentation estimate yet.
    assert_eq!(queue.content(), 2);
    assert!(!queue.ready_to_drain(20));
    queue.publish_presentation(30);
    assert!(!queue.ready_to_drain(29));
    assert!(queue.ready_to_drain(30));
    Ok(())
}
