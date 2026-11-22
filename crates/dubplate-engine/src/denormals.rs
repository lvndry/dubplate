//! Flushing denormal numbers to zero on the audio thread.
//!
//! A filter or a reverb fed silence decays towards zero through denormal numbers, the tiny
//! floats below `f32::MIN_POSITIVE`. Many processors compute with them tens of times slower
//! than with ordinary floats, enough to make a quiet passage overrun the callback. While a
//! [`FlushDenormals`] guard lives, the processor treats them as zero instead, as every
//! professional audio engine does on its audio thread. Effects already flush their own feedback
//! paths; this covers everything else, the EQ, the filters, the limiter and the time-stretcher.

/// Sets the processor to flush denormal numbers to zero on this thread until it is dropped,
/// then puts back the floating-point mode the thread had.
#[must_use = "denormals are flushed only while the guard lives"]
pub(crate) struct FlushDenormals {
    previous: Option<FloatControl>,
}

impl FlushDenormals {
    pub(crate) fn new() -> Self {
        let previous = float_control::read();
        if let Some(previous) = previous {
            float_control::write(previous.with_denormals_flushed());
        }
        Self { previous }
    }
}

impl Drop for FlushDenormals {
    fn drop(&mut self) {
        if let Some(previous) = self.previous {
            float_control::write(previous);
        }
    }
}

/// The bits of the processor's floating-point control register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloatControl(u64);

#[cfg(target_arch = "aarch64")]
mod float_control {
    use std::arch::asm;

    use super::FloatControl;

    /// FPCR.FZ: denormal inputs and results are flushed to zero.
    const FLUSH_TO_ZERO: u64 = 1 << 24;

    impl FloatControl {
        pub(super) fn with_denormals_flushed(self) -> Self {
            Self(self.0 | FLUSH_TO_ZERO)
        }

        #[cfg(test)]
        pub(super) fn flushes_denormals(self) -> bool {
            self.0 & FLUSH_TO_ZERO != 0
        }
    }

    pub(super) fn read() -> Option<FloatControl> {
        let value: u64;
        // SAFETY: reading FPCR has no side effect and touches no memory.
        unsafe {
            asm!("mrs {value}, fpcr", value = out(reg) value, options(nomem, nostack, preserves_flags));
        }
        Some(FloatControl(value))
    }

    pub(super) fn write(control: FloatControl) {
        // SAFETY: the value comes from `read`, with at most the flush-to-zero bit added, so only
        // how denormal numbers are rounded changes. No memory is touched.
        unsafe {
            asm!("msr fpcr, {value}", value = in(reg) control.0, options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod float_control {
    use std::arch::asm;

    use super::FloatControl;

    /// MXCSR.FTZ: denormal results are flushed to zero.
    const FLUSH_TO_ZERO: u64 = 1 << 15;
    /// MXCSR.DAZ: denormal inputs are read as zero.
    const DENORMALS_ARE_ZERO: u64 = 1 << 6;

    impl FloatControl {
        pub(super) fn with_denormals_flushed(self) -> Self {
            Self(self.0 | FLUSH_TO_ZERO | DENORMALS_ARE_ZERO)
        }

        #[cfg(test)]
        pub(super) fn flushes_denormals(self) -> bool {
            self.0 & FLUSH_TO_ZERO != 0
        }
    }

    pub(super) fn read() -> Option<FloatControl> {
        let mut value: u32 = 0;
        // SAFETY: `stmxcsr` stores the 4-byte MXCSR into `value`, a live, aligned local.
        unsafe {
            asm!("stmxcsr [{pointer}]", pointer = in(reg) &mut value, options(nostack, preserves_flags));
        }
        Some(FloatControl(u64::from(value)))
    }

    pub(super) fn write(control: FloatControl) {
        let value = u32::try_from(control.0).unwrap_or_default();
        // SAFETY: `ldmxcsr` loads MXCSR from `value`, a live, aligned local. The value comes
        // from `read` with at most the denormal bits added, so only how denormal numbers are
        // handled changes.
        unsafe {
            asm!("ldmxcsr [{pointer}]", pointer = in(reg) &value, options(nostack, readonly, preserves_flags));
        }
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
mod float_control {
    use super::FloatControl;

    impl FloatControl {
        pub(super) fn with_denormals_flushed(self) -> Self {
            self
        }

        #[cfg(test)]
        pub(super) fn flushes_denormals(self) -> bool {
            false
        }
    }

    pub(super) fn read() -> Option<FloatControl> {
        None
    }

    pub(super) fn write(_control: FloatControl) {}
}

#[cfg(all(test, any(target_arch = "aarch64", target_arch = "x86_64")))]
mod tests {
    use std::hint::black_box;

    use super::*;

    #[test]
    fn the_guard_flushes_denormals_while_it_lives_and_restores_the_mode_after() {
        let before = float_control::read().unwrap();
        assert!(!before.flushes_denormals());
        let denormal = black_box(f32::MIN_POSITIVE);
        assert!((denormal * black_box(0.5)).is_subnormal());
        {
            let _guard = FlushDenormals::new();
            assert!(float_control::read().unwrap().flushes_denormals());
            assert_eq!(denormal * black_box(0.5), 0.0);
        }
        assert_eq!(float_control::read().unwrap(), before);
        assert!((denormal * black_box(0.5)).is_subnormal());
    }
}
