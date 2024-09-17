use std::time::Duration;

use super::rr_from_bpm;

/// A snippet whose intended use is to drive simple avatar effects (like left/right ears twitching)
/// using the changes in the user's heart rate (or more specifically, the interval between beats).
///
/// If an RR Duration (or BPM conversion if RR isn't available) changes more than the set threshold
/// compared to the last compared one, then a flag is flipped depending on if RR raised or lowered.
pub struct Twitcher {
    twitch_threshold: f32,
    latest_rr: Duration,
    use_real_rr: bool,
}

impl Twitcher {
    pub fn new(twitch_threshold: f32) -> Self {
        Self {
            twitch_threshold,
            latest_rr: Duration::from_secs(1),
            use_real_rr: false,
        }
    }

    /// Returns: (twitch_up, twitch_down)
    ///
    /// twitch_up - RR has increased (BPM has *lowered*)
    ///
    /// twitch_down - RR has decreased (BPM has *raised*)
    pub fn handle(&mut self, bpm: u16, rr_intervals: &[Duration]) -> (bool, bool) {
        if !rr_intervals.is_empty() {
            self.use_real_rr = true;
        }
        let mut twitch_up = false;
        let mut twitch_down = false;
        let rr_intervals = if self.use_real_rr {
            rr_intervals.to_vec()
        } else {
            vec![rr_from_bpm(bpm)]
        };

        for new_rr in rr_intervals {
            // Duration.abs_diff() is nightly only for now, agh
            if (new_rr.as_secs_f32() - self.latest_rr.as_secs_f32()).abs() > self.twitch_threshold {
                twitch_up |= new_rr > self.latest_rr;
                twitch_down |= new_rr < self.latest_rr;
            }
            self.latest_rr = new_rr;
        }

        (twitch_up, twitch_down)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_bpm_when_rr_used() {
        let twitch_threshold = Duration::from_millis(50).as_secs_f32();
        let mut twitcher = Twitcher::new(twitch_threshold);

        // Initial check, BPM only
        let mut bpm = 60;
        let mut rr_intervals = Vec::new();
        let mut output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));
        // BPM raising should trigger lower RR return
        bpm = 80;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, true));
        // Should ignore BPM changes from now on,
        // only responding to real RR data
        bpm = 60;
        rr_intervals = vec![rr_from_bpm(90)];
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, true));
        bpm = 100;
        rr_intervals = vec![rr_from_bpm(50)];
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (true, false));
        bpm = 50;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));
        bpm = 30;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));
        bpm = 70;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));
    }
    #[test]
    fn bpm_only() {
        let twitch_threshold = Duration::from_millis(50).as_secs_f32();
        let mut twitcher = Twitcher::new(twitch_threshold);

        let mut bpm = 60;
        let rr_intervals = Vec::new();
        let mut output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));

        // BPM falling should trigger rising RR return
        bpm = 40;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (true, false));
        bpm = 40;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));
        bpm = 41;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));

        bpm = 50;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, true));
        bpm = 60;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, true));
        bpm = 70;
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, true));
    }
    #[test]
    fn multiple_rr_intervals() {
        let twitch_threshold = Duration::from_millis(50).as_secs_f32();
        let mut twitcher = Twitcher::new(twitch_threshold);

        let bpm = 60;
        let mut rr_intervals = vec![rr_from_bpm(60)];
        let mut output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));

        rr_intervals = vec![rr_from_bpm(60), rr_from_bpm(61)];
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, false));

        rr_intervals = vec![rr_from_bpm(60), rr_from_bpm(70)];
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (false, true));

        rr_intervals = vec![rr_from_bpm(70), rr_from_bpm(60)];
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (true, false));
        // Big changes in both directions should trigger both twitches
        rr_intervals = vec![rr_from_bpm(20), rr_from_bpm(200)];
        output = twitcher.handle(bpm, &rr_intervals);
        assert_eq!(output, (true, true));
    }
}
