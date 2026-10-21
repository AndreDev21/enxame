//! Relógio lógico híbrido (HLC): milissegundos físicos + contador (spec 7.1).
//!
//! Garante que o carimbo de uma mensagem seja maior que o de qualquer mensagem que o autor já
//! tenha visto, mesmo com relógios de parede atrasados ou voltando.

use serde::{Deserialize, Serialize};

/// Carimbo HLC. A ordem derivada é lexicográfica (`ms`, depois `counter`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Hlc {
    pub ms: u64,
    pub counter: u32,
}

impl Hlc {
    pub const ZERO: Hlc = Hlc { ms: 0, counter: 0 };

    pub fn new(ms: u64, counter: u32) -> Self {
        Self { ms, counter }
    }
}

/// Relógio de um autor. O tempo físico é injetado, o que torna o comportamento testável.
#[derive(Debug, Clone, Copy, Default)]
pub struct HlcClock {
    last: Hlc,
}

impl HlcClock {
    /// Retoma de um carimbo conhecido (o maior já presente no banco), para ser monotônico
    /// entre reinícios.
    pub fn resume_from(last: Hlc) -> Self {
        Self { last }
    }

    /// Carimbo para um evento local.
    pub fn tick(&mut self, now_ms: u64) -> Hlc {
        self.last = if now_ms > self.last.ms {
            Hlc::new(now_ms, 0)
        } else {
            Hlc::new(self.last.ms, self.last.counter.saturating_add(1))
        };
        self.last
    }

    /// Incorpora um carimbo recebido; o próximo `tick` será maior que ele.
    pub fn observe(&mut self, remote: Hlc, now_ms: u64) {
        let max = self.last.max(remote).max(Hlc::new(now_ms, 0));
        self.last = max;
    }

    pub fn last(&self) -> Hlc {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_follows_physical_time() {
        let mut c = HlcClock::default();
        assert_eq!(c.tick(100), Hlc::new(100, 0));
        assert_eq!(c.tick(250), Hlc::new(250, 0));
    }

    #[test]
    fn tick_is_monotonic_when_the_wall_clock_stalls_or_goes_back() {
        let mut c = HlcClock::default();
        let a = c.tick(1000);
        let b = c.tick(1000);
        let d = c.tick(900);
        assert!(a < b && b < d);
        assert_eq!(d, Hlc::new(1000, 2));
    }

    #[test]
    fn observe_pulls_the_clock_past_remote_stamps() {
        let mut c = HlcClock::default();
        c.tick(100);
        c.observe(Hlc::new(5000, 7), 200);
        let next = c.tick(300);
        assert!(next > Hlc::new(5000, 7));
    }

    #[test]
    fn resume_keeps_monotonicity_across_restarts() {
        let mut c = HlcClock::resume_from(Hlc::new(9000, 3));
        assert!(c.tick(100) > Hlc::new(9000, 3));
    }

    #[test]
    fn ordering_is_ms_then_counter() {
        assert!(Hlc::new(1, 9) < Hlc::new(2, 0));
        assert!(Hlc::new(2, 0) < Hlc::new(2, 1));
    }
}
