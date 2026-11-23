/// Coefficients recomputed only when their inputs actually move, for filters whose tick runs
/// at sample rate but whose controls settle to a few decimal places.
///
/// The inputs are compared exactly and start marked as unseen with NaN, which is never equal
/// to itself, so a fresh cache recomputes before the first tick even though its controls are
/// already settled.
#[derive(Clone, Debug)]
pub(crate) struct CachedCoefficients<S, V> {
    inputs: (f32, f32, S),
    value: V,
}

impl<S, V> CachedCoefficients<S, V>
where
    S: PartialEq + Copy,
{
    pub(crate) fn new(slope: S, initial: V) -> Self {
        Self {
            inputs: (f32::NAN, f32::NAN, slope),
            value: initial,
        }
    }

    /// Marks the first input as unseen, so the next tick recomputes: the coefficients depend
    /// on the sample rate, which the inputs do not carry.
    pub(crate) fn invalidate(&mut self) {
        self.inputs.0 = f32::NAN;
    }

    /// The cached value, recomputed if `inputs` differ from the last computation.
    #[inline]
    pub(crate) fn current(&mut self, inputs: (f32, f32, S), compute: impl FnOnce() -> V) -> &V {
        if inputs != self.inputs {
            self.inputs = inputs;
            self.value = compute();
        }
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::CachedCoefficients;
    use crate::dsp::filters::SvfCoefficients;

    fn pair(cutoff: f32) -> (SvfCoefficients, SvfCoefficients) {
        (
            SvfCoefficients::new(cutoff, 0.7, 48_000.0),
            SvfCoefficients::new(cutoff, 0.9, 48_000.0),
        )
    }

    #[test]
    fn a_fresh_cache_computes_once_and_caches_equal_inputs() {
        use std::cell::RefCell;
        let count = RefCell::new(0);
        let compute = || {
            *count.borrow_mut() += 1;
            pair(1_000.0)
        };
        let mut cache = CachedCoefficients::new("slope", pair(1_000.0));
        cache.current((1.0, 0.3, "slope"), compute);
        assert_eq!(*count.borrow(), 1, "NaN inputs mark a fresh cache as dirty");
        cache.current((1.0, 0.3, "slope"), compute);
        assert_eq!(*count.borrow(), 1, "equal inputs keep the cached value");
        cache.current((1.0, 0.5, "slope"), compute);
        assert_eq!(*count.borrow(), 2, "changed inputs recompute");
        cache.current((1.0, 0.5, "slope"), compute);
        assert_eq!(*count.borrow(), 2);
        cache.invalidate();
        assert!(cache.inputs.0.is_nan());
        cache.current((1.0, 0.5, "slope"), compute);
        assert_eq!(
            *count.borrow(),
            3,
            "an invalidated cache recomputes even on equal inputs"
        );
    }
}
