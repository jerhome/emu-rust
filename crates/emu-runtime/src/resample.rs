//! Core-rate → device-rate stereo resampling with dynamic rate control (ADR 0003). Pure code: no device,
//! no allocation once `out` has grown, fully unit-testable in a container without sound.

/// Bounds accepted by `StereoResampler::set_rate_adjust`; DRC normally stays within ±0.5 %.
pub const MAX_RATE_ADJUST: f64 = 0.02;

/// Linear-interpolating resampler for interleaved stereo. Keeps the last input frame and the fractional read
/// position between calls, so splitting the input into arbitrary chunks gives the same output as one call.
#[derive(Debug, Clone)]
pub struct StereoResampler {
    // Placeholder until runtime-av implements it (rates, adjust, position, previous frame).
    _todo: (),
}

impl StereoResampler {
    /// Rates in Hz. Non-finite or non-positive rates are replaced by 44 100 Hz with a warning (never panics).
    pub fn new(input_rate: f64, output_rate: f64) -> Self {
        let _ = (input_rate, output_rate);
        todo!()
    }

    /// Core rate changed (`SET_SYSTEM_AV_INFO`); the interpolation state is kept, no click.
    pub fn set_input_rate(&mut self, input_rate: f64) {
        let _ = input_rate;
        todo!()
    }

    /// Multiplies the nominal ratio by `adjust`, clamped to `1 ± MAX_RATE_ADJUST`. `> 1` produces more output.
    pub fn set_rate_adjust(&mut self, adjust: f64) {
        let _ = adjust;
        todo!()
    }

    /// Effective output frames per input frame (`output_rate / input_rate × adjust`).
    pub fn ratio(&self) -> f64 {
        todo!()
    }

    /// Converts interleaved stereo `i16` at the input rate into interleaved stereo `f32` in `[-1, 1]` at the
    /// output rate, **appended** to `out`. A trailing odd sample is ignored.
    pub fn process(&mut self, input: &[i16], out: &mut Vec<f32>) {
        let _ = (input, out);
        todo!()
    }

    /// Forgets the interpolation state (after a long stall or a stream restart).
    pub fn reset(&mut self) {
        todo!()
    }
}

/// Dynamic rate control: ratio multiplier for a ring-buffer fill level `fill` in `[0, 1]` (clamped).
/// `1 + max_deviation` when empty, `1` at half, `1 - max_deviation` when full (linear in between).
pub fn rate_adjust(fill: f64, max_deviation: f64) -> f64 {
    let _ = (fill, max_deviation);
    todo!()
}
