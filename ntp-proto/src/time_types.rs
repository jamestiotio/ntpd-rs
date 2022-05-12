use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};
use std::time::{Duration, Instant};

/// NtpInstant is a monotonically increasing value modelling the uptime of the NTP service
///
/// It is used to validate packets that we send out, and to order internal operations.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub struct NtpInstant {
    instant: Instant,
}

impl NtpInstant {
    pub fn now() -> Self {
        Self {
            instant: Instant::now(),
        }
    }

    // used to populate the T1 and T3 fields to ensure the server is doing something sensible
    // see `generate_poll_message` for details
    pub(crate) const fn to_bits(self) -> [u8; 8] {
        // self.instant.to_bits()
        panic!()
    }
}

impl Sub for NtpInstant {
    type Output = NtpDuration;

    fn sub(self, rhs: Self) -> Self::Output {
        let duration = if self.instant > rhs.instant {
            self.instant - rhs.instant
        } else {
            rhs.instant - self.instant
        };

        NtpDuration::from_system_duration(duration)
    }
}

impl Add<Duration> for NtpInstant {
    type Output = NtpInstant;

    fn add(mut self, rhs: Duration) -> Self::Output {
        self.instant += rhs;

        self
    }
}

/// NtpTimestamp represents an ntp timestamp without the era number.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Default)]
pub struct NtpTimestamp {
    timestamp: u64,
}

impl NtpTimestamp {
    pub(crate) const fn from_bits(bits: [u8; 8]) -> NtpTimestamp {
        NtpTimestamp {
            timestamp: u64::from_be_bytes(bits),
        }
    }

    pub(crate) const fn to_bits(self) -> [u8; 8] {
        self.timestamp.to_be_bytes()
    }

    /// Create an NTP timestamp from the number of seconds and nanoseconds that have
    /// passed since the last ntp era boundary.
    pub const fn from_seconds_nanos_since_ntp_era(seconds: u32, nanos: u32) -> Self {
        // Although having a valid interpretation, providing more
        // than 1 second worth of nanoseconds as input probably
        // indicates an error from the caller.
        debug_assert!(nanos < 1_000_000_000);
        // NTP uses 1/2^32 sec as its unit of fractional time.
        // our time is in nanoseconds, so 1/1e9 seconds
        let fraction = ((nanos as u64) << 32) / 1_000_000_000;

        // alternatively, abuse FP arithmetic to save an instruction
        // let fraction = (nanos as f64 * 4.294967296) as u64;

        let timestamp = ((seconds as u64) << 32) + fraction;
        NtpTimestamp::from_bits(timestamp.to_be_bytes())
    }

    #[cfg(any(test, feature = "fuzz"))]
    pub(crate) const fn from_fixed_int(timestamp: u64) -> NtpTimestamp {
        NtpTimestamp { timestamp }
    }
}

impl Add<NtpDuration> for NtpTimestamp {
    type Output = NtpTimestamp;

    fn add(self, rhs: NtpDuration) -> Self::Output {
        // In order to properly deal with ntp era changes, timestamps
        // need to roll over. Converting the duration to u64 here
        // still gives desired effects because of how two's complement
        // arithmetic works.
        NtpTimestamp {
            timestamp: self.timestamp.wrapping_add(rhs.duration as u64),
        }
    }
}

impl AddAssign<NtpDuration> for NtpTimestamp {
    fn add_assign(&mut self, rhs: NtpDuration) {
        // In order to properly deal with ntp era changes, timestamps
        // need to roll over. Converting the duration to u64 here
        // still gives desired effects because of how two's complement
        // arithmetic works.
        self.timestamp = self.timestamp.wrapping_add(rhs.duration as u64);
    }
}

impl Sub for NtpTimestamp {
    type Output = NtpDuration;

    fn sub(self, rhs: Self) -> Self::Output {
        // In order to properly deal with ntp era changes, timestamps
        // need to roll over. Doing a wrapping substract to a signed
        // integer type always gives us the result as if the eras of
        // the timestamps were chosen to minimize the norm of the
        // difference, which is the desired behaviour
        NtpDuration {
            duration: self.timestamp.wrapping_sub(rhs.timestamp) as i64,
        }
    }
}

impl Sub<NtpDuration> for NtpTimestamp {
    type Output = NtpTimestamp;

    fn sub(self, rhs: NtpDuration) -> Self::Output {
        // In order to properly deal with ntp era changes, timestamps
        // need to roll over. Converting the duration to u64 here
        // still gives desired effects because of how two's complement
        // arithmetic works.
        NtpTimestamp {
            timestamp: self.timestamp.wrapping_sub(rhs.duration as u64),
        }
    }
}

impl SubAssign<NtpDuration> for NtpTimestamp {
    fn sub_assign(&mut self, rhs: NtpDuration) {
        // In order to properly deal with ntp era changes, timestamps
        // need to roll over. Converting the duration to u64 here
        // still gives desired effects because of how two's complement
        // arithmetic works.
        self.timestamp = self.timestamp.wrapping_sub(rhs.duration as u64);
    }
}

/// NtpDuration is used to represent signed intervals between NtpTimestamps.
/// A negative duration interval is interpreted to mean that the first
/// timestamp used to define the interval represents a point in time after
/// the second timestamp.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Default)]
pub struct NtpDuration {
    duration: i64,
}

impl NtpDuration {
    pub const ZERO: Self = Self { duration: 0 };
    pub(crate) const ONE: Self = Self { duration: 1 << 32 };

    /// NtpDuration::from_seconds(16.0)
    pub(crate) const MAX_DISPERSION: Self = Self {
        duration: 68719476736,
    };

    /// NtpDuration::from_seconds(0.005)
    pub(crate) const MIN_DISPERSION: Self = Self { duration: 21474836 };

    pub(crate) const fn from_bits(bits: [u8; 8]) -> Self {
        Self {
            duration: i64::from_be_bytes(bits),
        }
    }

    pub(crate) const fn from_bits_short(bits: [u8; 4]) -> Self {
        NtpDuration {
            duration: (u32::from_be_bytes(bits) as i64) << 16,
        }
    }

    pub(crate) const fn to_bits_short(self) -> [u8; 4] {
        // serializing negative durations should never happen
        // and indicates a programming error elsewhere.
        // as for duration that are too large, saturating is
        // the safe option.
        assert!(self.duration >= 0);

        // Although saturating is safe to do, it probably still
        // should never happen in practice, so ensure we will
        // see it when running in debug mode.
        debug_assert!(self.duration <= 0x0000FFFFFFFFFFFF);

        match self.duration > 0x0000FFFFFFFFFFFF {
            true => 0xFFFFFFFF_u32,
            false => ((self.duration & 0x0000FFFFFFFF0000) >> 16) as u32,
        }
        .to_be_bytes()
    }

    /// Convert to an f64; required for statistical calculations
    /// (e.g. in clock filtering)
    pub fn to_seconds(self) -> f64 {
        // dividing by u32::MAX moves the decimal point to the right position
        self.duration as f64 / u32::MAX as f64
    }

    pub(crate) fn from_seconds(seconds: f64) -> Self {
        let i = seconds.floor();
        let f = seconds - i;

        // Ensure proper saturating behaviour
        let duration = match i as i64 {
            i if i >= std::i32::MIN as i64 && i <= std::i32::MAX as i64 => {
                (i << 32) | (f * u32::MAX as f64) as i64
            }
            i if i < std::i32::MIN as i64 => std::i64::MIN,
            i if i > std::i32::MAX as i64 => std::i64::MAX,
            _ => unreachable!(),
        };

        Self { duration }
    }

    /// Get the number of seconds (first return value) and nanoseconds
    /// (second return value) representing the length of this duration.
    /// The number of nanoseconds is guaranteed to be positiv and less
    /// than 10^9
    pub const fn as_seconds_nanos(self) -> (i32, u32) {
        (
            (self.duration >> 32) as i32,
            (((self.duration & 0xFFFFFFFF) * 1_000_000_000) >> 32) as u32,
        )
    }

    /// Interpret an exponent `k` as `2^k` seconds, expressed as an NtpDuration
    pub fn from_exponent(input: i8) -> Self {
        Self {
            duration: match input {
                exp if exp > 30 => std::i64::MAX,
                exp if exp > 0 && exp <= 30 => 0x1_0000_0000_i64 << exp,
                exp if exp <= 0 && exp >= -32 => 0x1_0000_0000_i64 >> -exp,
                _ => 0,
            },
        }
    }

    pub fn from_system_duration(duration: Duration) -> Self {
        let seconds = duration.as_secs();
        let nanos = duration.subsec_nanos();
        // Although having a valid interpretation, providing more
        // than 1 second worth of nanoseconds as input probably
        // indicates an error from the caller.
        debug_assert!(nanos < 1_000_000_000);
        // NTP uses 1/2^32 sec as its unit of fractional time.
        // our time is in nanoseconds, so 1/1e9 seconds
        let fraction = ((nanos as u64) << 32) / 1_000_000_000;

        // alternatively, abuse FP arithmetic to save an instruction
        // let fraction = (nanos as f64 * 4.294967296) as u64;

        let timestamp = ((seconds as u64) << 32) + fraction;
        NtpDuration::from_bits(timestamp.to_be_bytes())
    }

    #[cfg(any(test, feature = "fuzz"))]
    pub(crate) const fn from_fixed_int(duration: i64) -> NtpDuration {
        NtpDuration { duration }
    }
}

impl Add for NtpDuration {
    type Output = NtpDuration;

    fn add(self, rhs: Self) -> Self::Output {
        // For duration, saturation is safer as that ensures
        // addition or substraction of two big durations never
        // unintentionally cancel, ensuring that filtering
        // can properly reject on the result.
        NtpDuration {
            duration: self.duration.saturating_add(rhs.duration),
        }
    }
}

impl AddAssign for NtpDuration {
    fn add_assign(&mut self, rhs: Self) {
        // For duration, saturation is safer as that ensures
        // addition or substraction of two big durations never
        // unintentionally cancel, ensuring that filtering
        // can properly reject on the result.
        self.duration = self.duration.saturating_add(rhs.duration);
    }
}

impl Sub for NtpDuration {
    type Output = NtpDuration;

    fn sub(self, rhs: Self) -> Self::Output {
        // For duration, saturation is safer as that ensures
        // addition or substraction of two big durations never
        // unintentionally cancel, ensuring that filtering
        // can properly reject on the result.
        NtpDuration {
            duration: self.duration.saturating_sub(rhs.duration),
        }
    }
}

impl SubAssign for NtpDuration {
    fn sub_assign(&mut self, rhs: Self) {
        // For duration, saturation is safer as that ensures
        // addition or substraction of two big durations never
        // unintentionally cancel, ensuring that filtering
        // can properly reject on the result.
        self.duration = self.duration.saturating_sub(rhs.duration);
    }
}

macro_rules! ntp_duration_scalar_mul {
    ($scalar_type:ty) => {
        impl Mul<NtpDuration> for $scalar_type {
            type Output = NtpDuration;

            fn mul(self, rhs: NtpDuration) -> NtpDuration {
                // For duration, saturation is safer as that ensures
                // addition or substraction of two big durations never
                // unintentionally cancel, ensuring that filtering
                // can properly reject on the result.
                NtpDuration {
                    duration: rhs.duration.saturating_mul(self as i64),
                }
            }
        }

        impl Mul<$scalar_type> for NtpDuration {
            type Output = NtpDuration;

            fn mul(self, rhs: $scalar_type) -> NtpDuration {
                // For duration, saturation is safer as that ensures
                // addition or substraction of two big durations never
                // unintentionally cancel, ensuring that filtering
                // can properly reject on the result.
                NtpDuration {
                    duration: self.duration.saturating_mul(rhs as i64),
                }
            }
        }

        impl MulAssign<$scalar_type> for NtpDuration {
            fn mul_assign(&mut self, rhs: $scalar_type) {
                // For duration, saturation is safer as that ensures
                // addition or substraction of two big durations never
                // unintentionally cancel, ensuring that filtering
                // can properly reject on the result.
                self.duration = self.duration.saturating_mul(rhs as i64);
            }
        }
    };
}

ntp_duration_scalar_mul!(i8);
ntp_duration_scalar_mul!(i16);
ntp_duration_scalar_mul!(i32);
ntp_duration_scalar_mul!(i64);
ntp_duration_scalar_mul!(isize);
ntp_duration_scalar_mul!(u8);
ntp_duration_scalar_mul!(u16);
ntp_duration_scalar_mul!(u32);
// u64 and usize deliberately excluded as they can result in overflows

macro_rules! ntp_duration_scalar_div {
    ($scalar_type:ty) => {
        impl Div<$scalar_type> for NtpDuration {
            type Output = NtpDuration;

            fn div(self, rhs: $scalar_type) -> NtpDuration {
                // No overflow risks for division
                NtpDuration {
                    duration: self.duration / (rhs as i64),
                }
            }
        }

        impl DivAssign<$scalar_type> for NtpDuration {
            fn div_assign(&mut self, rhs: $scalar_type) {
                // No overflow risks for division
                self.duration /= (rhs as i64);
            }
        }
    };
}

ntp_duration_scalar_div!(i8);
ntp_duration_scalar_div!(i16);
ntp_duration_scalar_div!(i32);
ntp_duration_scalar_div!(i64);
ntp_duration_scalar_div!(isize);
ntp_duration_scalar_div!(u8);
ntp_duration_scalar_div!(u16);
ntp_duration_scalar_div!(u32);
// u64 and usize deliberately excluded as they can result in overflows

/// Stores when we will next exchange packages with a remote server.
//
// The value is in seconds stored in log2 format:
//
// - a value of 4 means 2^4 = 16 seconds
// - a value of 17 is 2^17 = ~36h
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PollInterval(i8);

impl PollInterval {
    // here we follow the spec (the code skeleton and ntpd repository use different values)
    pub const MIN: Self = Self(4);
    pub const MAX: Self = Self(17);

    #[must_use]
    pub fn inc(self) -> Self {
        Self(self.0 + 1).min(Self::MAX)
    }

    #[must_use]
    pub fn dec(self) -> Self {
        Self(self.0 - 1).max(Self::MIN)
    }

    pub const fn as_log(self) -> i8 {
        self.0
    }

    pub const fn as_duration(self) -> NtpDuration {
        NtpDuration {
            duration: 1 << (self.0 + 32),
        }
    }

    pub const fn as_system_duration(self) -> Duration {
        Duration::from_secs(1 << self.0)
    }
}

impl Default for PollInterval {
    fn default() -> Self {
        Self(4)
    }
}

/// Frequency tolerance PHI (unit: seconds per second)
#[derive(Clone, Copy)]
pub struct FrequencyTolerance {
    ppm: u32,
}

impl FrequencyTolerance {
    pub const fn ppm(ppm: u32) -> Self {
        Self { ppm }
    }
}

impl Mul<FrequencyTolerance> for NtpDuration {
    type Output = NtpDuration;

    fn mul(self, rhs: FrequencyTolerance) -> Self::Output {
        (self * rhs.ppm) / 1_000_000
    }
}

#[cfg(feature = "fuzz")]
pub fn fuzz_duration_from_seconds(v: f64) {
    if v.is_finite() {
        let duration = NtpDuration::from_seconds(v);
        assert!(v.signum() as i64 * duration.duration.signum() >= 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_sub() {
        let a = NtpTimestamp::from_fixed_int(5);
        let b = NtpTimestamp::from_fixed_int(3);
        assert_eq!(a - b, NtpDuration::from_fixed_int(2));
        assert_eq!(b - a, NtpDuration::from_fixed_int(-2));
    }

    #[test]
    fn test_timestamp_era_change() {
        let mut a = NtpTimestamp::from_fixed_int(1);
        let b = NtpTimestamp::from_fixed_int(0xFFFFFFFFFFFFFFFF);
        assert_eq!(a - b, NtpDuration::from_fixed_int(2));
        assert_eq!(b - a, NtpDuration::from_fixed_int(-2));

        let c = NtpDuration::from_fixed_int(2);
        let d = NtpDuration::from_fixed_int(-2);
        assert_eq!(b + c, a);
        assert_eq!(b - d, a);
        assert_eq!(a - c, b);
        assert_eq!(a + d, b);

        a -= c;
        assert_eq!(a, b);
        a += c;
        assert_eq!(a, NtpTimestamp::from_fixed_int(1));
    }

    #[test]
    fn test_timestamp_from_seconds_nanos() {
        assert_eq!(
            NtpTimestamp::from_seconds_nanos_since_ntp_era(0, 500_000_000),
            NtpTimestamp::from_fixed_int(0x80000000)
        );
        assert_eq!(
            NtpTimestamp::from_seconds_nanos_since_ntp_era(1, 0),
            NtpTimestamp::from_fixed_int(1 << 32)
        );
    }

    #[test]
    fn test_timestamp_duration_math() {
        let mut a = NtpTimestamp::from_fixed_int(5);
        let b = NtpDuration::from_fixed_int(2);
        assert_eq!(a + b, NtpTimestamp::from_fixed_int(7));
        assert_eq!(a - b, NtpTimestamp::from_fixed_int(3));
        a += b;
        assert_eq!(a, NtpTimestamp::from_fixed_int(7));
        a -= b;
        assert_eq!(a, NtpTimestamp::from_fixed_int(5));
    }

    #[test]
    fn test_duration_as_seconds_nanos() {
        assert_eq!(
            NtpDuration::from_fixed_int(0x80000000).as_seconds_nanos(),
            (0, 500_000_000)
        );
        assert_eq!(
            NtpDuration::from_fixed_int(1 << 33).as_seconds_nanos(),
            (2, 0)
        );
    }

    #[test]
    fn test_duration_math() {
        let mut a = NtpDuration::from_fixed_int(5);
        let b = NtpDuration::from_fixed_int(2);
        assert_eq!(a + b, NtpDuration::from_fixed_int(7));
        assert_eq!(a - b, NtpDuration::from_fixed_int(3));
        a += b;
        assert_eq!(a, NtpDuration::from_fixed_int(7));
        a -= b;
        assert_eq!(a, NtpDuration::from_fixed_int(5));
    }

    macro_rules! ntp_duration_scaling_test {
        ($name:ident, $scalar_type:ty) => {
            #[test]
            fn $name() {
                let mut a = NtpDuration::from_fixed_int(31);
                let b: $scalar_type = 2;
                assert_eq!(a * b, NtpDuration::from_fixed_int(62));
                assert_eq!(b * a, NtpDuration::from_fixed_int(62));
                assert_eq!(a / b, NtpDuration::from_fixed_int(15));
                a /= b;
                assert_eq!(a, NtpDuration::from_fixed_int(15));
                a *= b;
                assert_eq!(a, NtpDuration::from_fixed_int(30));
            }
        };
    }

    ntp_duration_scaling_test!(ntp_duration_scaling_i8, i8);
    ntp_duration_scaling_test!(ntp_duration_scaling_i16, i16);
    ntp_duration_scaling_test!(ntp_duration_scaling_i32, i32);
    ntp_duration_scaling_test!(ntp_duration_scaling_i64, i64);
    ntp_duration_scaling_test!(ntp_duration_scaling_isize, isize);
    ntp_duration_scaling_test!(ntp_duration_scaling_u8, u8);
    ntp_duration_scaling_test!(ntp_duration_scaling_u16, u16);
    ntp_duration_scaling_test!(ntp_duration_scaling_u32, u32);

    macro_rules! assert_eq_epsilon {
        ($a:expr, $b:expr, $epsilon:expr) => {
            assert!(
                ($a - $b).abs() < $epsilon,
                "Left not nearly equal to right:\nLeft: {}\nRight: {}\n",
                $a,
                $b
            );
        };
    }

    #[test]
    fn duration_seconds_roundtrip() {
        assert_eq_epsilon!(NtpDuration::from_seconds(0.0).to_seconds(), 0.0, 1e-9);
        assert_eq_epsilon!(NtpDuration::from_seconds(1.0).to_seconds(), 1.0, 1e-9);
        assert_eq_epsilon!(NtpDuration::from_seconds(1.5).to_seconds(), 1.5, 1e-9);
        assert_eq_epsilon!(NtpDuration::from_seconds(2.0).to_seconds(), 2.0, 1e-9);
    }

    #[test]
    fn duration_from_exponent() {
        assert_eq_epsilon!(NtpDuration::from_exponent(0).to_seconds(), 1.0, 1e-9);

        assert_eq_epsilon!(NtpDuration::from_exponent(1).to_seconds(), 2.0, 1e-9);

        assert_eq_epsilon!(
            NtpDuration::from_exponent(17).to_seconds(),
            2.0f64.powi(17),
            1e-4 // Less precision due to larger exponent
        );

        assert_eq_epsilon!(NtpDuration::from_exponent(-1).to_seconds(), 0.5, 1e-9);

        assert_eq_epsilon!(
            NtpDuration::from_exponent(-5).to_seconds(),
            1.0 / 2.0f64.powi(5),
            1e-9
        );
    }

    #[test]
    fn duration_from_exponent_reasonable() {
        for i in -32..=127 {
            assert!(NtpDuration::from_exponent(i) > NtpDuration::from_fixed_int(0));
        }
        for i in -128..-32 {
            NtpDuration::from_exponent(i); // should not crash
        }
    }

    #[test]
    fn duration_from_float_seconds_saturates() {
        assert_eq!(
            NtpDuration::from_seconds(1e40),
            NtpDuration::from_fixed_int(std::i64::MAX)
        );
        assert_eq!(
            NtpDuration::from_seconds(-1e40),
            NtpDuration::from_fixed_int(std::i64::MIN)
        );
    }

    #[test]
    fn poll_interval_clamps() {
        let mut interval = PollInterval::default();
        for _ in 0..100 {
            interval = interval.inc();
            assert!(interval <= PollInterval::MAX);
        }
        for _ in 0..100 {
            interval = interval.dec();
            assert!(interval >= PollInterval::MIN);
        }
        for _ in 0..100 {
            interval = interval.inc();
            assert!(interval <= PollInterval::MAX);
        }
    }

    #[test]
    fn poll_interval_to_duration() {
        assert_eq!(
            PollInterval(4).as_duration(),
            NtpDuration::from_fixed_int(16 << 32)
        );
        assert_eq!(
            PollInterval(5).as_duration(),
            NtpDuration::from_fixed_int(32 << 32)
        );

        let mut interval = PollInterval::default();
        for _ in 0..100 {
            assert_eq!(
                interval.as_duration().as_seconds_nanos().0,
                interval.as_system_duration().as_secs() as i32
            );
            interval = interval.inc();
        }

        for _ in 0..100 {
            assert_eq!(
                interval.as_duration().as_seconds_nanos().0,
                interval.as_system_duration().as_secs() as i32
            );
            interval = interval.dec();
        }
    }

    #[test]
    fn frequency_tolerance() {
        assert_eq!(
            NtpDuration::from_seconds(1.0),
            NtpDuration::from_seconds(1.0) * FrequencyTolerance::ppm(1_000_000),
        );
    }
}
