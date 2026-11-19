//! Core-rate → device-rate stereo resampling with dynamic rate control (ADR 0003). Pure code: no device,
//! no allocation once `out` has grown, fully unit-testable in a container without sound.

use tracing::warn;

/// Bounds accepted by `StereoResampler::set_rate_adjust`; DRC normally stays within ±0.5 %.
pub const MAX_RATE_ADJUST: f64 = 0.02;

/// Accepted sample-rate range in Hz; the core's rate is untrusted and extreme ratios would OOM or hang.
pub const MIN_RATE: f64 = 1_000.0;
pub const MAX_RATE: f64 = 1_000_000.0;
const FALLBACK_RATE: f64 = 44_100.0;
const I16_SCALE: f32 = 1.0 / 32_768.0;

/// Linear-interpolating resampler for interleaved stereo. Keeps the last input frame and the fractional read
/// position between calls, so splitting the input into arbitrary chunks gives the same output as one call.
#[derive(Debug, Clone)]
pub struct StereoResampler {
    input_rate: f64,
    output_rate: f64,
    adjust: f64,
    /// Input frames advanced per output frame (`1 / ratio`), cached so `process` does no division.
    step: f64,
    /// Read position in `[0, 1)` between `prev` and the next input frame.
    pos: f64,
    prev: [f32; 2],
    /// `false` until the first frame (or after `reset`): that frame then interpolates against itself, so the
    /// output starts on the signal instead of ramping up from silence.
    primed: bool,
}

fn sanitize_rate(rate: f64, what: &str) -> f64 {
    if (MIN_RATE..=MAX_RATE).contains(&rate) {
        rate
    } else {
        warn!("invalid audio {what} rate {rate}, using {FALLBACK_RATE} Hz");
        FALLBACK_RATE
    }
}

impl StereoResampler {
    /// Rates in Hz. Rates outside [1 000 ; 1 000 000] (or NaN) are replaced by 44 100 Hz with a warning (never panics).
    pub fn new(input_rate: f64, output_rate: f64) -> Self {
        let mut resampler = Self {
            input_rate: sanitize_rate(input_rate, "input"),
            output_rate: sanitize_rate(output_rate, "output"),
            adjust: 1.0,
            step: 1.0,
            pos: 0.0,
            prev: [0.0; 2],
            primed: false,
        };
        resampler.update_step();
        resampler
    }

    fn update_step(&mut self) {
        self.step = 1.0 / self.ratio();
    }

    /// Core rate changed (`SET_SYSTEM_AV_INFO`); the interpolation state is kept, no click.
    pub fn set_input_rate(&mut self, input_rate: f64) {
        self.input_rate = sanitize_rate(input_rate, "input");
        self.update_step();
    }

    /// Multiplies the nominal ratio by `adjust`, clamped to `1 ± MAX_RATE_ADJUST`. `> 1` produces more output.
    pub fn set_rate_adjust(&mut self, adjust: f64) {
        let adjust = if adjust.is_finite() { adjust } else { 1.0 };
        self.adjust = adjust.clamp(1.0 - MAX_RATE_ADJUST, 1.0 + MAX_RATE_ADJUST);
        self.update_step();
    }

    /// Effective output frames per input frame (`output_rate / input_rate × adjust`).
    pub fn ratio(&self) -> f64 {
        self.output_rate / self.input_rate * self.adjust
    }

    /// Converts interleaved stereo `i16` at the input rate into interleaved stereo `f32` in `[-1, 1]` at the
    /// output rate, **appended** to `out`. A trailing odd sample is ignored.
    pub fn process(&mut self, input: &[i16], out: &mut Vec<f32>) {
        let frames = input.len() / 2;
        if frames == 0 {
            return;
        }
        // Defensive: `pos` must stay finite or the inner loop would never end.
        if !self.pos.is_finite() {
            self.pos = 0.0;
        }
        // Upper bound of the output length; a no-op once `out` has enough capacity.
        let expected = (frames as f64 * self.ratio()).ceil() as usize + 1;
        out.reserve(expected * 2);

        for frame in input.as_chunks::<2>().0.iter() {
            let cur = [
                f32::from(frame[0]) * I16_SCALE,
                f32::from(frame[1]) * I16_SCALE,
            ];
            if !self.primed {
                self.prev = cur;
                self.primed = true;
            }
            while self.pos < 1.0 {
                let t = self.pos as f32;
                out.push(self.prev[0] + (cur[0] - self.prev[0]) * t);
                out.push(self.prev[1] + (cur[1] - self.prev[1]) * t);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }

    /// Forgets the interpolation state (after a long stall or a stream restart).
    pub fn reset(&mut self) {
        self.pos = 0.0;
        self.prev = [0.0; 2];
        self.primed = false;
    }
}

/// Dynamic rate control: ratio multiplier for a ring-buffer fill level `fill` in `[0, 1]` (clamped).
/// `1 + max_deviation` when empty, `1` at half, `1 - max_deviation` when full (linear in between).
pub fn rate_adjust(fill: f64, max_deviation: f64) -> f64 {
    if !fill.is_finite() || !max_deviation.is_finite() {
        return 1.0;
    }
    1.0 + max_deviation * (1.0 - 2.0 * fill.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_const(frames: usize, l: i16, r: i16) -> Vec<i16> {
        (0..frames).flat_map(|_| [l, r]).collect()
    }

    #[test]
    fn one_second_ratio() {
        let mut rs = StereoResampler::new(32_040.0, 48_000.0);
        let mut out = Vec::new();
        rs.process(&stereo_const(32_040, 100, -100), &mut out);
        assert_eq!(out.len() % 2, 0);
        let frames = (out.len() / 2) as i64;
        assert!((frames - 48_000).abs() <= 1, "{frames}");
    }

    #[test]
    fn chunking_is_exact() {
        let input: Vec<i16> = (0..20_000i32)
            .map(|i| ((i * 7919) % 65_536 - 32_768) as i16)
            .collect();
        let mut whole = Vec::new();
        StereoResampler::new(44_100.0, 48_000.0).process(&input, &mut whole);

        let mut rs = StereoResampler::new(44_100.0, 48_000.0);
        let mut split = Vec::new();
        let mut rest = &input[..];
        for size in [2usize, 0, 6, 1000, 2, 734, 4096].iter().cycle() {
            if rest.is_empty() {
                break;
            }
            let n = (*size).min(rest.len());
            rs.process(&rest[..n], &mut split);
            rest = &rest[n..];
        }
        assert_eq!(whole, split);
    }

    #[test]
    fn constant_signal_preserved() {
        let mut rs = StereoResampler::new(32_040.0, 48_000.0);
        let mut out = Vec::new();
        rs.process(&stereo_const(1000, 16_384, -8_192), &mut out);
        for frame in out.as_chunks::<2>().0.iter() {
            assert!((frame[0] - 0.5).abs() < 1e-6);
            assert!((frame[1] + 0.25).abs() < 1e-6);
        }
    }

    fn zero_crossings(samples: impl Iterator<Item = f32>) -> usize {
        let mut prev: Option<f32> = None;
        let mut count = 0;
        for s in samples {
            if let Some(p) = prev
                && (p < 0.0) != (s < 0.0)
            {
                count += 1;
            }
            prev = Some(s);
        }
        count
    }

    #[test]
    fn sine_frequency_preserved() {
        let in_rate = 32_040.0;
        let input: Vec<i16> = (0..32_040)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * 1000.0 * f64::from(i) / in_rate).sin();
                let s = (v * 20_000.0) as i16;
                [s, s]
            })
            .collect();
        let mut out = Vec::new();
        StereoResampler::new(in_rate, 48_000.0).process(&input, &mut out);
        let left = out.iter().step_by(2).copied();
        let crossings = zero_crossings(left) as f64;
        // 1 kHz over 1 s → 2000 zero crossings.
        assert!((crossings - 2000.0).abs() <= 20.0, "{crossings}");
    }

    #[test]
    fn trailing_odd_sample_ignored() {
        let mut a = Vec::new();
        let mut b = Vec::new();
        StereoResampler::new(32_000.0, 48_000.0).process(&[1, 2, 3, 4, 5], &mut a);
        StereoResampler::new(32_000.0, 48_000.0).process(&[1, 2, 3, 4], &mut b);
        assert_eq!(a, b);
        let mut c = Vec::new();
        StereoResampler::new(32_000.0, 48_000.0).process(&[7], &mut c);
        assert!(c.is_empty());
    }

    #[test]
    fn input_rate_change_is_continuous() {
        // Ramp signal: any discontinuity would show as a jump larger than the per-output slope.
        let input: Vec<i16> = (0..4000i32)
            .flat_map(|i| [(i * 4) as i16, (i * 4) as i16])
            .collect();
        let mut rs = StereoResampler::new(32_000.0, 48_000.0);
        let mut out = Vec::new();
        rs.process(&input[..4000], &mut out);
        rs.set_input_rate(44_100.0);
        rs.process(&input[4000..], &mut out);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let max_step = 4.0 / 32_768.0 + 1e-6;
        for w in left.windows(2) {
            let d = w[1] - w[0];
            assert!((-1e-6..=max_step).contains(&d), "jump {d}");
        }
    }

    #[test]
    fn ratio_follows_adjust_and_clamp() {
        let mut rs = StereoResampler::new(32_000.0, 48_000.0);
        assert!((rs.ratio() - 1.5).abs() < 1e-12);
        rs.set_rate_adjust(1.004);
        assert!((rs.ratio() - 1.5 * 1.004).abs() < 1e-12);
        rs.set_rate_adjust(1.5);
        assert!((rs.ratio() - 1.5 * (1.0 + MAX_RATE_ADJUST)).abs() < 1e-12);
        rs.set_rate_adjust(0.0);
        assert!((rs.ratio() - 1.5 * (1.0 - MAX_RATE_ADJUST)).abs() < 1e-12);
        rs.set_rate_adjust(f64::NAN);
        assert!((rs.ratio() - 1.5).abs() < 1e-12);
    }

    #[test]
    fn invalid_rates_fall_back() {
        let rs = StereoResampler::new(f64::NAN, -1.0);
        assert!((rs.ratio() - 1.0).abs() < 1e-12);
        let mut rs = StereoResampler::new(0.0, 48_000.0);
        assert!((rs.ratio() - 48_000.0 / 44_100.0).abs() < 1e-12);
        rs.set_input_rate(f64::INFINITY);
        assert!((rs.ratio() - 48_000.0 / 44_100.0).abs() < 1e-12);
    }

    #[test]
    fn out_of_range_rates_are_rejected_and_recoverable() {
        let fallback = 48_000.0 / FALLBACK_RATE;
        for bad in [
            1e-6,
            f64::MIN_POSITIVE / 2.0,
            0.0,
            f64::NAN,
            f64::INFINITY,
            2e6,
        ] {
            let rs = StereoResampler::new(bad, 48_000.0);
            assert!((rs.ratio() - fallback).abs() < 1e-12, "{bad}");
            let mut rs = StereoResampler::new(32_000.0, 48_000.0);
            rs.set_input_rate(bad);
            assert!((rs.ratio() - fallback).abs() < 1e-12, "{bad}");
            let mut out = Vec::new();
            rs.process(&stereo_const(441, 100, 100), &mut out);
            assert!(!out.is_empty());
            rs.set_input_rate(32_000.0);
            out.clear();
            rs.process(&stereo_const(32_000, 100, 100), &mut out);
            let frames = (out.len() / 2) as i64;
            assert!((frames - 48_000).abs() <= 2, "{bad}: {frames}");
        }
    }

    #[test]
    fn reset_restarts_cleanly() {
        let input = stereo_const(100, 1000, 1000);
        let mut fresh = Vec::new();
        StereoResampler::new(32_000.0, 48_000.0).process(&input, &mut fresh);
        let mut rs = StereoResampler::new(32_000.0, 48_000.0);
        let mut out = Vec::new();
        rs.process(&stereo_const(33, -5, 9), &mut out);
        rs.reset();
        out.clear();
        rs.process(&input, &mut out);
        assert_eq!(out, fresh);
    }

    #[test]
    fn rate_adjust_curve() {
        let d = 0.005;
        assert!((rate_adjust(0.0, d) - 1.005).abs() < 1e-12);
        assert!((rate_adjust(0.5, d) - 1.0).abs() < 1e-12);
        assert!((rate_adjust(1.0, d) - 0.995).abs() < 1e-12);
        assert!((rate_adjust(0.25, d) - 1.0025).abs() < 1e-12);
        assert!((rate_adjust(-3.0, d) - 1.005).abs() < 1e-12);
        assert!((rate_adjust(7.0, d) - 0.995).abs() < 1e-12);
        assert!((rate_adjust(f64::NAN, d) - 1.0).abs() < 1e-12);
    }
}
