//! [`Rng`]: `xoshiro256**` seeded through `SplitMix64`. The simulation needs one reproducible
//! stream per seed, not unpredictability, so a dozen lines replace a dependency.

pub(crate) struct Rng([u64; 4]);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        let mut state = seed;
        let mut next = || {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        Self([next(), next(), next(), next()])
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = &mut self.0;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = *s1 << 17;
        *s2 ^= *s0;
        *s3 ^= *s1;
        *s1 ^= *s2;
        *s0 ^= *s3;
        *s2 ^= t;
        *s3 = s3.rotate_left(45);
        result
    }

    /// Uniform in [0, 1).
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// Uniform in [low, high).
    pub(crate) fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    /// Uniform in [0, n); `n` is small, so the modulo's bias does not matter.
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// Standard normal (Box–Muller).
    pub(crate) fn normal(&mut self) -> f64 {
        let u = 1.0 - self.unit();
        let v = self.unit();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }

    /// Exponential with mean `mean`.
    pub(crate) fn exp(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.unit()).ln()
    }

    pub(crate) fn byte(&mut self) -> u8 {
        self.next_u64().to_le_bytes()[0]
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn a_seed_gives_one_stream() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        let first: Vec<_> = (0..8).map(|_| a.next_u64()).collect();
        assert_eq!(first, (0..8).map(|_| b.next_u64()).collect::<Vec<_>>());
        assert_ne!(first[0], Rng::new(8).next_u64());
    }

    #[test]
    fn draws_stay_in_range() {
        let mut rng = Rng::new(1);
        for _ in 0..10_000 {
            let unit = rng.unit();
            assert!((0.0..1.0).contains(&unit));
            assert!(rng.below(5) < 5);
            assert!(rng.exp(10.0) >= 0.0);
        }
        let mean = (0..10_000).map(|_| rng.normal()).sum::<f64>() / 10_000.0;
        assert!(mean.abs() < 0.05, "{mean}");
    }
}
