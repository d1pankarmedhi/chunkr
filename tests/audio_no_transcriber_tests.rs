//! Audio without a registered transcriber fails with instructions, not a guess.

#![cfg(all(feature = "audio", not(target_arch = "wasm32")))]

use chunkr::audio::{encode_wav, load_documents_with};

#[test]
fn loading_audio_without_a_transcriber_explains_the_fix() {
    let mut samples = Vec::new();
    for index in 0..16_000 {
        let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / 16_000.0;
        samples.push(phase.sin() * 0.3);
    }
    let wav = encode_wav(&samples, 16_000);

    let error = load_documents_with(&wav, "wav", Some("episode.wav"), None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no transcriber registered"), "{error}");
    assert!(error.contains("chunkr-audio"), "{error}");
    assert!(error.contains("register_transcriber"), "{error}");
}
