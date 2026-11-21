//! The host process's clock (the change `sluice-host-runs-on-a-tokio-edge`,
//! design D5): the core's `Clock` seam read from tokio's clock, so a test
//! that pauses tokio's clock and moves it moves every time the core reads
//! in the host process too.

use sluice_core::Clock;
use tokio::time::Instant;

/// The wall time at one instant of tokio's clock, and tokio's clock after
/// it.
#[derive(Clone, Copy, Debug)]
pub struct TokioClock {
    wall: chrono::DateTime<chrono::Utc>,
    at: Instant,
}

impl TokioClock {
    /// A clock that reads the wall time now, and moves as tokio's clock
    /// moves.
    pub fn new() -> TokioClock {
        TokioClock {
            wall: chrono::Utc::now(),
            at: Instant::now(),
        }
    }
}

impl Default for TokioClock {
    fn default() -> TokioClock {
        TokioClock::new()
    }
}

impl Clock for TokioClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.wall + chrono::TimeDelta::from_std(self.at.elapsed()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// D5: on a paused clock, the core's time moves only as a test moves
    /// tokio's clock.
    #[tokio::test(start_paused = true)]
    async fn the_cores_time_moves_with_tokios_clock() {
        let clock = TokioClock::new();
        let first = clock.now();
        assert_eq!(clock.now(), first);
        tokio::time::advance(Duration::from_secs(59)).await;
        assert_eq!(clock.now() - first, chrono::TimeDelta::seconds(59));
    }
}
