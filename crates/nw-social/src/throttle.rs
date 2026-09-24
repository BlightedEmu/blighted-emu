// SPDX-License-Identifier: AGPL-3.0-or-later
//! The chat rate limit: the values of the player prefab's `ChatComponent`
//! (`slices/player.dynamicslice`, SharedDataStrm-part8.pak: `m_minTimeBetweenChatMsgsMS` 500,
//! `m_chatThrottleWindowSize` 10, `m_numThrottledChatMessages` 4). The client keeps the same
//! limit and shows `@ui_error_chatthrottle` itself, so a line over it is only a client that does
//! not keep it; the server drops such a line.
//!
//! The window unit is not in the slice; it is taken as seconds (Inferred from the 500 ms gap).

use std::collections::VecDeque;

/// Least time between two lines of one player.
pub const MIN_GAP_MS: u64 = 500;
/// The window of the line count.
pub const WINDOW_MS: u64 = 10_000;
/// Most lines of one player in one window.
pub const WINDOW_LINES: usize = 4;

/// The recent lines of one player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatThrottle {
    sent_ms: VecDeque<u64>,
}

impl ChatThrottle {
    /// Whether a line at `now_ms` keeps the limit; when it does, it counts.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        while self
            .sent_ms
            .front()
            .is_some_and(|sent| now_ms.saturating_sub(*sent) >= WINDOW_MS)
        {
            self.sent_ms.pop_front();
        }
        let too_soon = self
            .sent_ms
            .back()
            .is_some_and(|last| now_ms.saturating_sub(*last) < MIN_GAP_MS);
        if too_soon || self.sent_ms.len() >= WINDOW_LINES {
            return false;
        }
        self.sent_ms.push_back(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_need_the_gap_and_fit_the_window() {
        let mut throttle = ChatThrottle::default();
        assert!(throttle.allow(0));
        assert!(!throttle.allow(MIN_GAP_MS - 1), "too soon");
        let lines: Vec<u64> = (1..WINDOW_LINES as u64)
            .map(|line| line * MIN_GAP_MS)
            .collect();
        for at in &lines {
            assert!(throttle.allow(*at));
        }
        let after = lines.last().copied().unwrap_or_default() + MIN_GAP_MS;
        assert!(!throttle.allow(after), "a fifth line in the window");
        assert!(throttle.allow(WINDOW_MS), "the first line left the window");
    }
}
