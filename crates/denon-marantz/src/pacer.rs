//! Spacing between requests to one receiver.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use crate::error::Error;

/// Answers slower than this say nothing about the speed of the receiver.
const SLOW_ANSWER: Duration = Duration::from_secs(2);

/// How requests are spaced.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Pacing {
    /// The same time between two requests.
    Fixed {
        /// Time between the start of two requests.
        interval: Duration,
    },
    /// The time follows how fast the receiver answers.
    Adaptive(Adaptive),
}

/// Settings of [`Pacing::Adaptive`].
///
/// The rate is `k` divided by the average answer time, kept between the
/// minimum and the maximum rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adaptive {
    /// Requests per second at the start.
    pub initial_rate: f64,
    /// Lowest requests per second.
    pub min_rate: f64,
    /// Highest requests per second.
    pub max_rate: f64,
    /// Scale of the rate.
    pub k: f64,
    /// Shortest time between two changes of the rate.
    pub adjust_interval: Duration,
    /// Smallest relative change of the rate that is applied.
    pub threshold: f64,
    /// Weight of the newest answer time in the average, above 0 and up to 1.
    pub alpha: f64,
}

impl Default for Adaptive {
    fn default() -> Self {
        Self {
            initial_rate: 25.0,
            min_rate: 5.0,
            max_rate: 100.0,
            k: 2.0,
            adjust_interval: Duration::from_secs(10),
            threshold: 0.2,
            alpha: 0.2,
        }
    }
}

impl Adaptive {
    fn check(&self) -> Result<(), Error> {
        if !(self.initial_rate > 0.0 && self.min_rate > 0.0 && self.max_rate > 0.0 && self.k > 0.0)
        {
            return Err(Error::InvalidConfig("rates and k must be above 0"));
        }
        if self.min_rate > self.max_rate {
            return Err(Error::InvalidConfig("min_rate must not be above max_rate"));
        }
        if !(self.alpha > 0.0 && self.alpha <= 1.0) {
            return Err(Error::InvalidConfig("alpha must be above 0 and up to 1"));
        }
        if self.threshold <= 0.0 || self.adjust_interval.is_zero() {
            return Err(Error::InvalidConfig(
                "threshold and adjust_interval must be above 0",
            ));
        }
        Ok(())
    }
}

impl Adaptive {
    fn target_rate(&self, average_seconds: f64) -> f64 {
        if average_seconds <= 0.0 {
            self.initial_rate
        } else {
            (self.k / average_seconds).clamp(self.min_rate, self.max_rate)
        }
    }
}

#[derive(Debug)]
struct Inner {
    interval: Duration,
    next: Instant,
    rate: f64,
    average_seconds: Option<f64>,
    last_adjust: Instant,
}

/// Makes callers wait so that requests keep the chosen spacing.
///
/// One value is shared by all callers that talk to one receiver. A request
/// that arrives after a pause is sent at once.
#[derive(Debug)]
pub struct Pacer {
    adaptive: Option<Adaptive>,
    inner: Mutex<Inner>,
}

impl Pacer {
    /// Creates a pacer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] if a setting is out of range.
    pub fn new(pacing: Pacing) -> Result<Self, Error> {
        let now = Instant::now();
        let (adaptive, rate, interval) = match pacing {
            Pacing::Fixed { interval } => (
                None,
                1.0 / interval.as_secs_f64().max(f64::MIN_POSITIVE),
                interval,
            ),
            Pacing::Adaptive(settings) => {
                settings.check()?;
                (
                    Some(settings),
                    settings.initial_rate,
                    Duration::from_secs_f64(1.0 / settings.initial_rate),
                )
            }
        };
        Ok(Self {
            adaptive,
            inner: Mutex::new(Inner {
                interval,
                next: now,
                rate,
                average_seconds: None,
                last_adjust: now,
            }),
        })
    }

    /// Time that a request waits at least after the one before it.
    #[must_use]
    pub fn interval(&self) -> Duration {
        self.lock().interval
    }

    /// Waits until a request may be sent.
    ///
    /// The slot is taken when the wait ends, so a call that is dropped while it
    /// waits delays no other call.
    pub async fn wait(&self) {
        loop {
            let next = {
                let mut inner = self.lock();
                let now = Instant::now();
                if inner.next <= now {
                    inner.next = now + inner.interval;
                    return;
                }
                inner.next
            };
            tokio::time::sleep_until(next).await;
        }
    }

    /// Reports how long an answer took, since `started`.
    ///
    /// Only adaptive pacing uses this.
    pub fn record_latency(&self, started: Instant) {
        let Some(settings) = &self.adaptive else {
            return;
        };
        let now = Instant::now();
        let seconds = now.saturating_duration_since(started);
        if seconds >= SLOW_ANSWER {
            return;
        }
        let seconds = seconds.as_secs_f64();
        let mut inner = self.lock();
        let average = inner.average_seconds.map_or(seconds, |old| {
            // Moves the average towards the new value by `alpha` of the distance.
            let step = settings.alpha * (seconds - old);
            old + step
        });
        inner.average_seconds = Some(average);
        if now.saturating_duration_since(inner.last_adjust) < settings.adjust_interval {
            return;
        }
        let rate = settings.target_rate(average);
        if (rate - inner.rate).abs() / inner.rate <= settings.threshold {
            return;
        }
        inner.last_adjust = now;
        inner.rate = rate;
        inner.interval = Duration::from_secs_f64(1.0 / rate);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[tokio::test(start_paused = true)]
    async fn fixed_interval_spaces_requests() {
        let pacer = Pacer::new(Pacing::Fixed { interval: ms(50) }).unwrap();
        let start = Instant::now();
        pacer.wait().await;
        assert_eq!(start.elapsed(), ms(0));
        pacer.wait().await;
        assert_eq!(start.elapsed(), ms(50));
        pacer.wait().await;
        assert_eq!(start.elapsed(), ms(100));
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_after_a_pause_is_sent_at_once() {
        let pacer = Pacer::new(Pacing::Fixed { interval: ms(50) }).unwrap();
        pacer.wait().await;
        tokio::time::sleep(ms(500)).await;
        let start = Instant::now();
        pacer.wait().await;
        assert_eq!(start.elapsed(), ms(0));
        pacer.wait().await;
        assert_eq!(start.elapsed(), ms(50));
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_wait_does_not_delay_the_next_call() {
        let pacer = Pacer::new(Pacing::Fixed { interval: ms(50) }).unwrap();
        let start = Instant::now();
        pacer.wait().await;
        for _ in 0..3 {
            let dropped = tokio::time::timeout(ms(1), pacer.wait()).await;
            assert!(dropped.is_err());
        }
        pacer.wait().await;
        assert_eq!(start.elapsed(), ms(50));
    }

    #[tokio::test(start_paused = true)]
    async fn waiting_callers_are_spaced_too() {
        let pacer = std::sync::Arc::new(Pacer::new(Pacing::Fixed { interval: ms(10) }).unwrap());
        let start = Instant::now();
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let pacer = pacer.clone();
                tokio::spawn(async move {
                    pacer.wait().await;
                    Instant::now()
                })
            })
            .collect();
        let mut times = Vec::new();
        for task in tasks {
            times.push(task.await.unwrap().duration_since(start));
        }
        times.sort();
        assert_eq!(times, [ms(0), ms(10), ms(20), ms(30)]);
    }

    #[tokio::test(start_paused = true)]
    async fn adaptive_starts_at_the_initial_rate() {
        let pacer = Pacer::new(Pacing::Adaptive(Adaptive::default())).unwrap();
        assert_eq!(pacer.interval(), ms(40));
    }

    #[tokio::test(start_paused = true)]
    async fn adaptive_slows_down_for_slow_answers() {
        let pacer = Pacer::new(Pacing::Adaptive(Adaptive::default())).unwrap();
        // Answers take 0.5 s, so the target is 2.0 / 0.5 = 4 per second.
        // That is below the minimum of 5 per second.
        tokio::time::advance(Duration::from_secs(11)).await;
        pacer.record_latency(Instant::now() - ms(500));
        assert_eq!(pacer.interval(), ms(200));
    }

    #[tokio::test(start_paused = true)]
    async fn adaptive_speeds_up_for_fast_answers() {
        let pacer = Pacer::new(Pacing::Adaptive(Adaptive::default())).unwrap();
        // Answers take 10 ms, so the target is 200 per second, capped at 100.
        tokio::time::advance(Duration::from_secs(11)).await;
        pacer.record_latency(Instant::now() - ms(10));
        assert_eq!(pacer.interval(), ms(10));
    }

    #[tokio::test(start_paused = true)]
    async fn adaptive_changes_are_limited() {
        let pacer = Pacer::new(Pacing::Adaptive(Adaptive::default())).unwrap();
        // Too early after the start.
        tokio::time::advance(Duration::from_secs(1)).await;
        pacer.record_latency(Instant::now() - ms(500));
        assert_eq!(pacer.interval(), ms(40));
        // Slow answers are ignored.
        tokio::time::advance(Duration::from_secs(11)).await;
        pacer.record_latency(Instant::now() - Duration::from_secs(3));
        assert_eq!(pacer.interval(), ms(40));
    }

    #[tokio::test(start_paused = true)]
    async fn adaptive_ignores_small_changes() {
        let pacer = Pacer::new(Pacing::Adaptive(Adaptive::default())).unwrap();
        // 2.0 / 0.08 = 25 per second, the same as the start.
        tokio::time::advance(Duration::from_secs(11)).await;
        pacer.record_latency(Instant::now() - ms(80));
        assert_eq!(pacer.interval(), ms(40));
    }

    #[tokio::test(start_paused = true)]
    async fn fixed_pacing_ignores_latency() {
        let pacer = Pacer::new(Pacing::Fixed { interval: ms(50) }).unwrap();
        tokio::time::advance(Duration::from_secs(60)).await;
        pacer.record_latency(Instant::now() - ms(500));
        assert_eq!(pacer.interval(), ms(50));
    }

    #[test]
    fn settings_are_checked() {
        let adaptive = |change: fn(&mut Adaptive)| {
            let mut settings = Adaptive::default();
            change(&mut settings);
            Pacer::new(Pacing::Adaptive(settings)).unwrap_err()
        };
        assert!(matches!(
            adaptive(|a| a.initial_rate = 0.0),
            Error::InvalidConfig(_)
        ));
        assert!(matches!(adaptive(|a| a.k = -1.0), Error::InvalidConfig(_)));
        assert!(matches!(
            adaptive(|a| a.min_rate = 200.0),
            Error::InvalidConfig(_)
        ));
        assert!(matches!(
            adaptive(|a| a.alpha = 0.0),
            Error::InvalidConfig(_)
        ));
        assert!(matches!(
            adaptive(|a| a.alpha = 1.5),
            Error::InvalidConfig(_)
        ));
        assert!(matches!(
            adaptive(|a| a.threshold = 0.0),
            Error::InvalidConfig(_)
        ));
        assert!(matches!(
            adaptive(|a| a.adjust_interval = Duration::ZERO),
            Error::InvalidConfig(_)
        ));
        assert!(matches!(
            adaptive(|a| a.initial_rate = f64::NAN),
            Error::InvalidConfig(_)
        ));
    }
}
