const DENORMAL_FLOOR: f32 = 1e-15;
const SAFETY_KNEE: f32 = 2.0;
const INPUT_LIMIT: f32 = 16.0;

/// Zeroes values so small they would turn into denormals in a feedback loop.
#[inline]
pub(crate) fn flush_denormal(value: f32) -> f32 {
    if value.abs() < DENORMAL_FLOOR {
        0.0
    } else {
        value
    }
}

/// A smooth saturation close to `tanh`: nearly linear for quiet signals, never beyond ±1.
#[inline]
pub(crate) fn saturate(value: f32) -> f32 {
    let clamped = value.clamp(-3.0, 3.0);
    let squared = clamped * clamped;
    clamped * (27.0 + squared) / (27.0 + 9.0 * squared)
}

/// Leaves values within ±`ceiling / 2` untouched and bends larger ones towards ±`ceiling`.
#[inline]
pub(crate) fn soft_limit(value: f32, ceiling: f32) -> f32 {
    let knee = ceiling * 0.5;
    let magnitude = value.abs();
    if magnitude <= knee {
        value
    } else {
        let excess = magnitude - knee;
        (knee + excess / (1.0 + excess / knee)).copysign(value)
    }
}

/// The last stage of every effect: no NaN or infinity, and nothing far beyond full scale.
#[inline]
pub(crate) fn sanitize_output(value: f32) -> f32 {
    if value.is_finite() {
        soft_limit(value, SAFETY_KNEE * 2.0)
    } else {
        0.0
    }
}

/// Replaces NaN and infinity with silence and caps absurd levels before audio enters an
/// effect, so no buffer or filter state can overflow.
#[inline]
pub(crate) fn sanitize_input(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-INPUT_LIMIT, INPUT_LIMIT)
    } else {
        0.0
    }
}

/// A cubic step from 0 to 1 with flat ends, for fades that should not be heard as corners.
#[inline]
pub(crate) fn smoothstep(value: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    clamped * clamped * (3.0 - 2.0 * clamped)
}

#[cfg(test)]
mod tests {
    use super::{sanitize_output, saturate, soft_limit};

    #[test]
    fn saturation_is_bounded_and_nearly_linear_when_quiet() {
        assert!((saturate(0.05) - 0.05).abs() < 1e-4);
        assert!(saturate(100.0) <= 1.0);
        assert!(saturate(-100.0) >= -1.0);
    }

    #[test]
    fn soft_limit_is_transparent_below_the_knee() {
        assert_eq!(soft_limit(0.9, 4.0), 0.9);
        assert_eq!(soft_limit(-1.9, 4.0), -1.9);
        assert!(soft_limit(1_000.0, 4.0) < 4.0);
        assert!(soft_limit(f32::MAX, 4.0) <= 4.0);
    }

    #[test]
    fn output_is_always_finite() {
        assert_eq!(sanitize_output(f32::NAN), 0.0);
        assert_eq!(sanitize_output(f32::INFINITY), 0.0);
        assert!(sanitize_output(f32::MAX) <= 4.0);
    }
}
