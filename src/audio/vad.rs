//! Speech detection over decoded samples.
//!
//! Retrieval does not need a neural VAD: the job here is to know *when* somebody
//! is talking, so silence-only files fail early and every transcript carries a
//! speech ratio. An energy detector over an adaptive noise floor does that with no
//! model artifact to ship, no ONNX runtime and nothing to download on first use.
//!
//! A model-based VAD (Silero and friends) can be added later behind the same
//! [`SpeechSpan`] output without touching the loader; the transcriber registry is
//! the extension point for anything smarter.

/// Span of detected speech, in seconds from the start of the audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechSpan {
    pub start_s: f64,
    pub end_s: f64,
}

impl SpeechSpan {
    pub fn duration_s(&self) -> f64 {
        (self.end_s - self.start_s).max(0.0)
    }
}

/// Tunables for [`speech_spans`].
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Analysis frame length in milliseconds.
    pub frame_ms: u32,
    /// How far above the noise floor a frame must be to count as speech, in dB.
    pub threshold_db: f32,
    /// Absolute floor: quieter than this is never speech, in dBFS.
    pub silence_db: f32,
    /// Spans shorter than this are dropped (a door slam, a click).
    pub min_speech_s: f64,
    /// Gaps shorter than this are closed, so words are not split on breath.
    pub min_silence_s: f64,
    /// Added to both ends of every span.
    pub padding_s: f64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            threshold_db: 9.0,
            // -50 dBFS: below this even loud speech in a quiet room is ~-30 dBFS.
            silence_db: -50.0,
            min_speech_s: 0.25,
            min_silence_s: 0.35,
            padding_s: 0.10,
        }
    }
}

/// Detect speech spans in mono `samples`.
///
/// The noise floor is the 10th percentile of frame energy, so a recording with
/// constant background hum still separates speech from silence; the threshold is
/// whichever is higher of `floor + threshold_db` and `silence_db`.
pub fn speech_spans(samples: &[f32], sample_rate: u32, config: &VadConfig) -> Vec<SpeechSpan> {
    let frame_len = ((sample_rate as f64 * config.frame_ms as f64) / 1000.0).round() as usize;
    if frame_len == 0 || samples.len() < frame_len {
        return Vec::new();
    }

    let energies: Vec<f32> = samples
        .chunks_exact(frame_len)
        .map(|frame| {
            let mean_square: f32 =
                frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32;
            10.0 * mean_square.max(f32::MIN_POSITIVE).log10()
        })
        .collect();
    if energies.is_empty() {
        return Vec::new();
    }

    let floor = percentile(&energies, 0.10);
    let mut threshold = (floor + config.threshold_db).max(config.silence_db);
    // A recording with no quiet frame at all (a voice memo, an edit with a music
    // bed under it) has its own signal as the noise floor, which would put the
    // threshold above everything and report silence. There is energy, so let the
    // loud part through and leave the judgement to the transcriber.
    let loudest = energies.iter().copied().fold(f32::MIN, f32::max);
    if threshold >= loudest {
        threshold = (loudest - 6.0).max(config.silence_db);
    }

    let frame_s = frame_len as f64 / sample_rate as f64;
    let mut spans: Vec<SpeechSpan> = Vec::new();
    let mut open: Option<f64> = None;

    for (index, energy) in energies.iter().enumerate() {
        let start = index as f64 * frame_s;
        if *energy >= threshold {
            open.get_or_insert(start);
        } else if let Some(begin) = open.take() {
            spans.push(SpeechSpan {
                start_s: begin,
                end_s: start,
            });
        }
    }
    if let Some(begin) = open {
        spans.push(SpeechSpan {
            start_s: begin,
            end_s: energies.len() as f64 * frame_s,
        });
    }

    merge_spans(spans, config)
}

fn merge_spans(spans: Vec<SpeechSpan>, config: &VadConfig) -> Vec<SpeechSpan> {
    let total_s = spans.last().map(|span| span.end_s).unwrap_or(0.0);
    let mut merged: Vec<SpeechSpan> = Vec::new();

    for span in spans {
        match merged.last_mut() {
            Some(previous) if span.start_s - previous.end_s < config.min_silence_s => {
                previous.end_s = span.end_s;
            }
            _ => merged.push(span),
        }
    }

    merged
        .into_iter()
        .filter(|span| span.duration_s() >= config.min_speech_s)
        .map(|span| SpeechSpan {
            start_s: (span.start_s - config.padding_s).max(0.0),
            end_s: (span.end_s + config.padding_s).min(total_s),
        })
        .collect()
}

fn percentile(values: &[f32], fraction: f64) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(samples: &mut Vec<f32>, seconds: f64, rate: u32, amplitude: f32) {
        let count = (seconds * rate as f64) as usize;
        for index in 0..count {
            let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / rate as f32;
            samples.push(phase.sin() * amplitude);
        }
    }

    fn silence(samples: &mut Vec<f32>, seconds: f64, rate: u32) {
        let count = (seconds * rate as f64) as usize;
        samples.extend(std::iter::repeat_n(0.0, count));
    }

    #[test]
    fn finds_speech_between_silences() {
        let rate = 16_000;
        let mut samples = Vec::new();
        silence(&mut samples, 1.0, rate);
        tone(&mut samples, 1.0, rate, 0.3);
        silence(&mut samples, 1.0, rate);
        tone(&mut samples, 0.8, rate, 0.3);
        silence(&mut samples, 0.5, rate);

        let spans = speech_spans(&samples, rate, &VadConfig::default());
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert!((spans[0].start_s - 0.9).abs() < 0.15, "{:?}", spans[0]);
        assert!((spans[0].end_s - 2.1).abs() < 0.15, "{:?}", spans[0]);
        assert!((spans[1].start_s - 2.9).abs() < 0.15, "{:?}", spans[1]);
    }

    #[test]
    fn silence_only_audio_has_no_spans() {
        let rate = 16_000;
        let mut samples = Vec::new();
        silence(&mut samples, 3.0, rate);
        assert!(speech_spans(&samples, rate, &VadConfig::default()).is_empty());
    }

    #[test]
    fn short_blips_are_dropped_and_near_gaps_joined() {
        let rate = 16_000;
        let mut samples = Vec::new();
        silence(&mut samples, 0.5, rate);
        tone(&mut samples, 0.05, rate, 0.3); // below min_speech_s
        silence(&mut samples, 0.5, rate);
        tone(&mut samples, 0.6, rate, 0.3);
        silence(&mut samples, 0.1, rate); // below min_silence_s
        tone(&mut samples, 0.6, rate, 0.3);
        silence(&mut samples, 0.5, rate);

        let spans = speech_spans(&samples, rate, &VadConfig::default());
        assert_eq!(spans.len(), 1, "blip dropped, gap joined: {spans:?}");
        assert!(spans[0].duration_s() > 1.2, "{spans:?}");
    }

    #[test]
    fn a_recording_without_silence_still_counts_as_speech() {
        let rate = 16_000;
        let mut samples = Vec::new();
        tone(&mut samples, 2.0, rate, 0.3);
        let spans = speech_spans(&samples, rate, &VadConfig::default());
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans[0].duration_s() > 1.8, "{spans:?}");
    }

    #[test]
    fn quiet_speech_above_a_constant_hum_is_still_found() {
        let rate = 16_000;
        let mut samples = Vec::new();
        // Constant hum at -46 dBFS for the whole file.
        let hum = 10f32.powf(-46.0 / 20.0);
        for index in 0..rate as usize * 3 {
            let phase = 2.0 * std::f32::consts::PI * 100.0 * index as f32 / rate as f32;
            samples.push(phase.sin() * hum);
        }
        // Speech bursts at -20 dBFS in the middle second.
        for index in 0..rate as usize {
            let slot = 2 * rate as usize + index;
            let phase = 2.0 * std::f32::consts::PI * 220.0 * index as f32 / rate as f32;
            samples[slot] += phase.sin() * 10f32.powf(-20.0 / 20.0);
        }

        let spans = speech_spans(&samples, rate, &VadConfig::default());
        assert_eq!(spans.len(), 1, "{spans:?}");
        // Speech occupies 2.0-3.0s; padding extends the span by 0.1s each side.
        assert!((spans[0].start_s - 1.9).abs() < 0.15, "{:?}", spans[0]);
        assert!((spans[0].end_s - 3.0).abs() < 0.15, "{:?}", spans[0]);
    }
}
