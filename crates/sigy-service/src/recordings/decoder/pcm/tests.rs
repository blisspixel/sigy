use super::{Samples, copy_pcm};
use tokio::io::AsyncWriteExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

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
