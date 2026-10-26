//! Cutting long audio where it hurts least.
//!
//! Speech models are trained on short inputs — 30 s for Whisper, 3,000 mel frames
//! for the Qwen audio encoders — and hosted APIs cap uploads by size. WhisperX
//! showed the right way to feed them: never cut in the middle of an active word,
//! cut where the signal is quietest (its "min-cut" searches the VAD score minimum
//! between half and the full model window; sglang does the same with RMS in the
//! last second before a seam).
//!
//! This module does that without a model: for each nominal seam it scores short
//! frames around it and cuts at the quietest one. Audio is *never* dropped and the
//! windows never overlap, so concatenating the transcripts gives the recording back
//! in order.

/// Analyser frame used to score candidate seams, in seconds.
const FRAME_S: f64 = 0.05;

/// Shortest window worth transcribing; keeps a pathological search from stalling.
const MIN_WINDOW_S: f64 = 1.0;

/// Split `samples` into windows of at most `max_s`, cutting each seam at the
/// quietest frame within `search_s` of the nominal boundary.
///
/// Returns contiguous `(start, end)` sample ranges covering the whole input.
pub fn window_bounds(samples: &[f32], rate: u32, max_s: f64, search_s: f64) -> Vec<(usize, usize)> {
    let frames = samples.len();
    if frames == 0 || rate == 0 {
        return Vec::new();
    }
    let rate = rate as f64;
    let max_samples = (max_s.max(MIN_WINDOW_S) * rate) as usize;
    if frames <= max_samples {
        return vec![(0, frames)];
    }

    let frame = ((FRAME_S * rate) as usize).max(1);
    let search = (search_s.max(0.0) * rate) as usize;
    let min_window = (MIN_WINDOW_S * rate) as usize;

    let mut bounds = Vec::new();
    let mut start = 0usize;
    while frames - start > max_samples {
        let nominal = start + max_samples;
        // Never cut so early that the window is shorter than the minimum, nor so
        // late that the remainder could not be transcribed.
        // Search *before* the nominal boundary: a seam after it would make the
        // window longer than the caller asked for, and cutting early is what
        // WhisperX's min-cut does too.
        let low = nominal.saturating_sub(search).max(start + min_window);
        let high = nominal.min(frames.saturating_sub(min_window)).max(low);
        let seam = quietest_seam(samples, low, high, frame).unwrap_or(nominal);
        let seam = seam
            .max(start + min_window)
            .min(frames.saturating_sub(min_window));
        if seam <= start {
            // The search found nothing usable; take the nominal boundary rather
            // than looping.
            bounds.push((start, nominal.min(frames)));
            start = nominal.min(frames);
            if start >= frames {
                break;
            }
            continue;
        }
        bounds.push((start, seam));
        start = seam;
    }
    if start < frames {
        bounds.push((start, frames));
    }
    bounds
}

/// Midpoint of the quietest frame in `[low, high)`, preferring the centre on ties.
fn quietest_seam(samples: &[f32], low: usize, high: usize, frame: usize) -> Option<usize> {
    if high <= low || samples.is_empty() {
        return None;
    }
    let centre = (low + high) as f64 / 2.0;
    let step = (frame / 2).max(1);
    let mut best: Option<(f32, f64, usize)> = None;
    let mut position = low;
    while position < high {
        let end = (position + frame).min(samples.len());
        let energy = frame_energy(samples, position, end);
        let midpoint = position + (end - position) / 2;
        let distance = (midpoint as f64 - centre).abs();
        let better = match best {
            None => true,
            Some((best_energy, best_distance, _)) => {
                energy < best_energy - 0.5
                    || ((energy - best_energy).abs() <= 0.5 && distance < best_distance)
            }
        };
        if better {
            best = Some((energy, distance, midpoint));
        }
        position += step;
    }
    best.map(|(_, _, midpoint)| midpoint.clamp(low, high))
}

/// Energy of `samples[from..to]`, in dBFS.
fn frame_energy(samples: &[f32], from: usize, to: usize) -> f32 {
    let slice = &samples[from.min(samples.len())..to.min(samples.len())];
    if slice.is_empty() {
        return f32::MIN;
    }
    let mean_square: f32 = slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32;
    10.0 * mean_square.max(f32::MIN_POSITIVE).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Speech bursts separated by quiet gaps; returns the samples and the gap centres.
    fn bursts(rate: u32, total_s: f64, gap_s: f64) -> (Vec<f32>, Vec<f64>) {
        let rate_f = rate as f64;
        let mut samples = vec![0.0f32; (total_s * rate_f) as usize];
        let mut gaps = Vec::new();
        // 2 s of speech then a gap: any ±2 s search window contains a gap.
        let mut position = 0.5;
        while position + gap_s < total_s {
            let gap_start = (position * rate_f) as usize;
            let gap_end = ((position + gap_s) * rate_f) as usize;
            let limit = samples.len();
            for sample in &mut samples[gap_start..gap_end.min(limit)] {
                *sample = 0.0;
            }
            gaps.push(position + gap_s / 2.0);
            position += gap_s + 2.0;
        }
        for (index, sample) in samples.iter_mut().enumerate() {
            let t = index as f64 / rate_f;
            // Loud tone everywhere except the declared gaps.
            if !gaps.iter().any(|gap| (t - gap).abs() < gap_s / 2.0 - 0.01) {
                *sample = (2.0 * std::f32::consts::PI * 220.0 * t as f32).sin() * 0.5;
            }
        }
        (samples, gaps)
    }

    #[test]
    fn windows_are_contiguous_and_cover_everything() {
        let rate = 16_000;
        let (samples, _) = bursts(rate, 20.0, 0.5);
        let bounds = window_bounds(&samples, rate, 6.0, 2.0);
        assert!(bounds.len() >= 3, "{bounds:?}");
        assert_eq!(bounds.first().unwrap().0, 0);
        assert_eq!(bounds.last().unwrap().1, samples.len());
        for pair in bounds.windows(2) {
            assert_eq!(
                pair[0].1, pair[1].0,
                "windows must not drop or repeat audio"
            );
        }
        for (start, end) in &bounds {
            let seconds = (end - start) as f64 / rate as f64;
            assert!(seconds <= 6.0 + 0.001, "window is {seconds:.2}s");
        }
    }

    #[test]
    fn seams_land_in_the_quiet_gaps() {
        let rate = 16_000;
        let (samples, gaps) = bursts(rate, 20.0, 0.5);
        let bounds = window_bounds(&samples, rate, 4.0, 2.0);
        for (_, seam) in bounds.iter().rev().skip(1) {
            let at = *seam as f64 / rate as f64;
            let nearest = gaps
                .iter()
                .map(|gap| (gap - at).abs())
                .fold(f64::MAX, f64::min);
            assert!(
                nearest < 0.6,
                "seam at {at:.2}s is not near a gap (closest {nearest:.2}s)"
            );
        }
    }

    #[test]
    fn a_short_recording_is_a_single_window() {
        let samples = vec![0.1f32; 16_000];
        assert_eq!(
            window_bounds(&samples, 16_000, 30.0, 2.0),
            vec![(0, 16_000)]
        );
    }

    #[test]
    fn constant_audio_still_terminates_and_covers_everything() {
        let samples = vec![0.3f32; 16_000 * 31];
        let bounds = window_bounds(&samples, 16_000, 5.0, 2.0);
        assert_eq!(bounds.first().unwrap().0, 0);
        assert_eq!(bounds.last().unwrap().1, samples.len());
        for pair in bounds.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
    }

    #[test]
    fn a_nonsense_max_is_raised_to_the_minimum() {
        let samples = vec![0.2f32; 16_000 * 5];
        let bounds = window_bounds(&samples, 16_000, 0.1, 0.0);
        for (start, end) in &bounds {
            assert!((end - start) >= 16_000, "windows must stay usable");
        }
        assert_eq!(bounds.last().unwrap().1, samples.len());
    }

    #[test]
    fn empty_and_degenerate_inputs_return_nothing() {
        assert!(window_bounds(&[], 16_000, 30.0, 2.0).is_empty());
        assert!(window_bounds(&[0.0; 100], 0, 30.0, 2.0).is_empty());
    }
}
