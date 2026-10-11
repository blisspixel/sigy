use super::{Samples, copy_pcm};
use tokio::io::AsyncWriteExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore = "requires the configured native FFmpeg and enforceable native groups"]
async fn direct_live_pcm_has_exact_stereo_samples_and_separate_finite_progress() -> TestResult {
    use crate::recordings::audio::{
        DirectPcmRequest, NativeAudioGroup, PcmFormat, decode_direct_pcm_reader,
    };
    let executable = std::env::var("SIGY_TEST_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
    let frames = 6000_u32;
    let wave = stereo_wave(frames);
    let group = NativeAudioGroup::for_decoder()?;
    let request = DirectPcmRequest {
        executable: &executable,
        input_format: "wav",
        format: PcmFormat {
            rate_hz: 48_000,
            channels: 2,
        },
        group: &group,
    };
    let (send, mut receive) = tokio::sync::mpsc::channel(2);
    let decode = decode_direct_pcm_reader(request, wave.as_slice(), send);
    let collect = async {
        let mut bytes = Vec::new();
        while let Some(chunk) = receive.recv().await {
            bytes.extend(chunk);
        }
        bytes
    };
    let (result, samples) = tokio::join!(decode, collect);
    let decoded = result?;
    assert_eq!(decoded.decoder.playhead_us, 125_000);
    assert_eq!(decoded.pcm_bytes, u64::from(frames) * 8);
    assert_eq!(samples.len(), usize::try_from(frames)? * 8);
    for frame in samples.as_chunks::<8>().0 {
        assert_eq!(frame[..4], 0.25_f32.to_le_bytes());
        assert_eq!(frame[4..], (-0.25_f32).to_le_bytes());
    }
    group.finish().await?;
    refusals(request, &wave).await
}

fn stereo_wave(frames: u32) -> Vec<u8> {
    let bytes = frames * 4;
    let mut wave = Vec::new();
    wave.extend(b"RIFF");
    wave.extend((36 + bytes).to_le_bytes());
    wave.extend(b"WAVEfmt ");
    wave.extend(16_u32.to_le_bytes());
    wave.extend(1_u16.to_le_bytes());
    wave.extend(2_u16.to_le_bytes());
    wave.extend(48_000_u32.to_le_bytes());
    wave.extend(192_000_u32.to_le_bytes());
    wave.extend(4_u16.to_le_bytes());
    wave.extend(16_u16.to_le_bytes());
    wave.extend(b"data");
    wave.extend(bytes.to_le_bytes());
    for _ in 0..frames {
        wave.extend(8192_i16.to_le_bytes());
        wave.extend((-8192_i16).to_le_bytes());
    }
    wave
}

async fn refusals(
    request: crate::recordings::audio::DirectPcmRequest<'_>,
    wave: &[u8],
) -> TestResult {
    use crate::recordings::audio::{DirectPcmRequest, NativeAudioGroup, decode_direct_pcm_reader};
    for format in ["http", "hls", "", "wav\n"] {
        let group = NativeAudioGroup::for_decoder()?;
        let (send, _receive) = tokio::sync::mpsc::channel(2);
        assert!(
            decode_direct_pcm_reader(
                DirectPcmRequest {
                    executable: "must-not-start",
                    input_format: format,
                    group: &group,
                    ..request
                },
                wave,
                send
            )
            .await
            .is_err()
        );
        group.finish().await?;
    }
    for data in [vec![], vec![0; 64]] {
        let group = NativeAudioGroup::for_decoder()?;
        let (send, _receive) = tokio::sync::mpsc::channel(2);
        assert!(
            decode_direct_pcm_reader(
                DirectPcmRequest {
                    group: &group,
                    ..request
                },
                data.as_slice(),
                send
            )
            .await
            .is_err()
        );
        group.finish().await?;
    }
    let group = NativeAudioGroup::for_decoder()?;
    let (send, _receive) = tokio::sync::mpsc::channel(1);
    assert!(
        decode_direct_pcm_reader(
            DirectPcmRequest {
                executable: "must-not-start",
                group: &group,
                ..request
            },
            wave,
            send
        )
        .await
        .is_err()
    );
    group.finish().await?;
    Ok(())
}

#[tokio::test]
async fn excerpt_pcm_preserves_split_scalars_and_signed_zero_with_exact_caps() -> TestResult {
    let bytes: Vec<u8> = [-0.0_f32, 0.0, -1.5, 1.5]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let (mut writer, reader) = tokio::io::duplex(1);
    let write = async {
        writer.write_all(&bytes).await?;
        writer.shutdown().await
    };
    let (send, mut receive) = tokio::sync::mpsc::channel(2);
    let copy = copy_pcm(reader, send, 16, true);
    let collect = async {
        let mut actual = Vec::new();
        while let Some(chunk) = receive.recv().await {
            actual.extend(chunk);
        }
        actual
    };
    let (written, count, actual) = tokio::join!(write, copy, collect);
    written?;
    assert_eq!(count?, 16);
    assert_eq!(actual, bytes);
    Ok(())
}

#[tokio::test]
async fn excerpt_pcm_refuses_nonfinite_partial_empty_and_overlong_input() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let (send, _receive) = tokio::sync::mpsc::channel(2);
        assert!(
            copy_pcm(value.to_le_bytes().as_slice(), send, 4, true)
                .await
                .is_err()
        );
    }
    for (bytes, cap) in [(vec![], 4), (vec![0; 3], 4), (vec![0; 8], 4)] {
        let (send, _receive) = tokio::sync::mpsc::channel(2);
        assert!(copy_pcm(bytes.as_slice(), send, cap, true).await.is_err());
    }
    let mut samples = Samples::default();
    assert!(samples.check(&[0, 0, 128]).is_ok());
    assert!(samples.check(&[127]).is_err());
    let (send, receive) = tokio::sync::mpsc::channel(2);
    drop(receive);
    assert!(copy_pcm([0; 4].as_slice(), send, 4, true).await.is_err());
}
