//! Pure playlist progression, shuffle queue, and search filter state.

use crate::config::RepeatMode;
use crate::scanner::Track;

/// Action produced when a playing track reaches its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionAction {
    Play(usize),
    Stop,
}

/// Manages track ordering, shuffle queue, repeat rules, and search filtering.
#[derive(Debug, Clone)]
pub struct PlaylistController {
    pub track_count: usize,
    pub current_track: Option<usize>,
    pub shuffle_indices: Vec<usize>,
    pub shuffle_cursor: usize,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub search_query: Option<String>,
    pub filtered_indices: Vec<usize>,
    rng_seed: u64,
}

impl PlaylistController {
    pub fn new(repeat: RepeatMode, shuffle: bool, rng_seed: u64) -> Self {
        Self {
            track_count: 0,
            current_track: None,
            shuffle_indices: Vec::new(),
            shuffle_cursor: 0,
            repeat,
            shuffle,
            search_query: None,
            filtered_indices: Vec::new(),
            rng_seed: if rng_seed == 0 { 0xCAFE_BABE } else { rng_seed },
        }
    }

    pub fn load_tracks(&mut self, count: usize) {
        self.track_count = count;
        self.current_track = None;
        self.search_query = None;
        self.filtered_indices = (0..count).collect();
        self.rebuild_shuffle_queue();
    }

    fn shuffle_vec(&mut self) {
        self.shuffle_indices = (0..self.track_count).collect();
        let n = self.shuffle_indices.len();
        if n > 1 {
            for i in (1..n).rev() {
                let j = (self.next_rand() as usize) % (i + 1);
                self.shuffle_indices.swap(i, j);
            }
        }
        self.shuffle_cursor = 0;
    }

    pub fn rebuild_shuffle_queue(&mut self) {
        self.shuffle_vec();
        if let Some(cur) = self.current_track {
            if let Some(pos) = self.shuffle_indices.iter().position(|&idx| idx == cur) {
                self.shuffle_indices.swap(0, pos);
            }
        }
    }

    fn rebuild_shuffle_queue_after_wrap(&mut self, last_played: Option<usize>) {
        self.shuffle_vec();
        if self.track_count > 1 && self.shuffle_indices.first().copied() == last_played {
            let swap_idx = 1 + (self.next_rand() as usize) % (self.track_count - 1);
            self.shuffle_indices.swap(0, swap_idx);
        }
    }

    pub fn sync_shuffle_cursor(&mut self, track_idx: usize) {
        if let Some(pos) = self.shuffle_indices.iter().position(|&idx| idx == track_idx) {
            self.shuffle_cursor = pos;
        }
    }

    pub fn next_on_completion(&mut self) -> CompletionAction {
        if self.track_count == 0 {
            return CompletionAction::Stop;
        }
        let Some(cur) = self.current_track else {
            return CompletionAction::Stop;
        };

        match self.repeat {
            RepeatMode::One => CompletionAction::Play(cur),
            RepeatMode::Once => CompletionAction::Stop,
            RepeatMode::All => self
                .next_manual()
                .map(CompletionAction::Play)
                .unwrap_or(CompletionAction::Stop),
            RepeatMode::Off => {
                if self.shuffle {
                    if self.shuffle_indices.len() != self.track_count {
                        self.rebuild_shuffle_queue();
                    }
                    if self.shuffle_cursor + 1 < self.shuffle_indices.len() {
                        self.shuffle_cursor += 1;
                        CompletionAction::Play(self.shuffle_indices[self.shuffle_cursor])
                    } else {
                        CompletionAction::Stop
                    }
                } else if cur + 1 < self.track_count {
                    CompletionAction::Play(cur + 1)
                } else {
                    CompletionAction::Stop
                }
            }
        }
    }

    pub fn handle_completion_failure(&mut self, failed_idx: usize) -> CompletionAction {
        if self.repeat == RepeatMode::One {
            return CompletionAction::Stop;
        }
        self.current_track = Some(failed_idx);
        self.next_on_completion()
    }

    pub fn next_manual(&mut self) -> Option<usize> {
        if self.track_count == 0 {
            return None;
        }

        if self.shuffle {
            if self.shuffle_indices.len() != self.track_count {
                self.rebuild_shuffle_queue();
            }
            if self.shuffle_cursor + 1 < self.shuffle_indices.len() {
                self.shuffle_cursor += 1;
            } else {
                let last_played = self.current_track;
                self.rebuild_shuffle_queue_after_wrap(last_played);
            }
            Some(self.shuffle_indices[self.shuffle_cursor])
        } else {
            let next_idx = match self.current_track {
                Some(idx) => (idx + 1) % self.track_count,
                None => 0,
            };
            Some(next_idx)
        }
    }

    pub fn prev_manual(&mut self) -> Option<usize> {
        if self.track_count == 0 {
            return None;
        }

        if self.shuffle {
            if self.shuffle_indices.len() != self.track_count {
                self.rebuild_shuffle_queue();
            }
            if self.shuffle_cursor == 0 {
                self.shuffle_cursor = self.shuffle_indices.len().saturating_sub(1);
            } else {
                self.shuffle_cursor -= 1;
            }
            Some(self.shuffle_indices[self.shuffle_cursor])
        } else {
            let prev_idx = match self.current_track {
                Some(idx) => {
                    if idx == 0 {
                        self.track_count - 1
                    } else {
                        idx - 1
                    }
                }
                None => 0,
            };
            Some(prev_idx)
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.rebuild_shuffle_queue();
        }
    }

    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.filtered_indices = (0..self.track_count).collect();
    }

    pub fn apply_search_filter(&mut self, tracks: &[Track]) {
        if let Some(ref q) = self.search_query {
            let q_lower = q.to_lowercase();
            self.filtered_indices = tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    t.file_name.to_lowercase().contains(&q_lower)
                        || t.artist.to_lowercase().contains(&q_lower)
                        || t.title.to_lowercase().contains(&q_lower)
                })
                .map(|(i, _)| i)
                .collect();
        } else {
            self.filtered_indices = (0..tracks.len()).collect();
        }
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng_seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_seed = x;
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repeat_once_stops_after_single_track() {
        let mut pc = PlaylistController::new(RepeatMode::Once, false, 42);
        pc.load_tracks(5);
        pc.current_track = Some(0);
        assert_eq!(pc.next_on_completion(), CompletionAction::Stop);

        pc.shuffle = true;
        pc.current_track = Some(2);
        assert_eq!(pc.next_on_completion(), CompletionAction::Stop);
    }

    #[test]
    fn test_repeat_off_with_shuffle_plays_all_once_then_stops() {
        let mut pc = PlaylistController::new(RepeatMode::Off, true, 12345);
        pc.load_tracks(4);
        pc.current_track = Some(pc.shuffle_indices[0]);

        let mut played = vec![pc.current_track.unwrap()];
        while let CompletionAction::Play(idx) = pc.next_on_completion() {
            pc.current_track = Some(idx);
            played.push(idx);
        }

        assert_eq!(played.len(), 4);
        let mut sorted = played.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_repeat_off_sequential_stops_at_last_track() {
        let mut pc = PlaylistController::new(RepeatMode::Off, false, 1);
        pc.load_tracks(3);
        pc.current_track = Some(0);
        assert_eq!(pc.next_on_completion(), CompletionAction::Play(1));
        pc.current_track = Some(1);
        assert_eq!(pc.next_on_completion(), CompletionAction::Play(2));
        pc.current_track = Some(2);
        assert_eq!(pc.next_on_completion(), CompletionAction::Stop);
    }

    #[test]
    fn test_repeat_one_always_replays_current_track() {
        let mut pc = PlaylistController::new(RepeatMode::One, false, 7);
        pc.load_tracks(4);
        pc.current_track = Some(2);
        assert_eq!(pc.next_on_completion(), CompletionAction::Play(2));
    }

    #[test]
    fn test_clear_search_restores_full_indices_and_load_tracks_clears_search() {
        let mut pc = PlaylistController::new(RepeatMode::All, false, 9);
        pc.load_tracks(5);
        pc.search_query = Some("rock".to_string());
        pc.filtered_indices = vec![2, 4];

        pc.clear_search();
        assert_eq!(pc.search_query, None);
        assert_eq!(pc.filtered_indices, vec![0, 1, 2, 3, 4]);

        pc.search_query = Some("jazz".to_string());
        pc.filtered_indices = vec![1];
        pc.load_tracks(3);
        assert_eq!(pc.search_query, None);
        assert_eq!(pc.filtered_indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_shuffle_wraparound_places_last_played_randomly_not_always_second() {
        let mut last_played_positions = Vec::new();

        for seed in 1u64..=24 {
            let mut pc = PlaylistController::new(RepeatMode::All, true, seed);
            pc.load_tracks(6);
            // Walk to the final track in the first shuffle pass
            for i in 0..5 {
                pc.current_track = Some(pc.shuffle_indices[i]);
                pc.shuffle_cursor = i;
            }
            let last_played = pc.shuffle_indices[5];
            pc.current_track = Some(last_played);
            pc.shuffle_cursor = 5;

            // Wrap around to the next shuffle pass
            let first_of_next = pc.next_manual().unwrap();
            assert_ne!(
                first_of_next, last_played,
                "wraparound must not immediately replay last_played"
            );

            let pos = pc
                .shuffle_indices
                .iter()
                .position(|&x| x == last_played)
                .unwrap();
            last_played_positions.push(pos);
        }

        // Before the fix, last_played was forced into index 1 on every single seed
        assert!(
            last_played_positions.iter().any(|&p| p != 1),
            "last_played was stuck at index 1 across all seeds: {:?}",
            last_played_positions
        );
    }

    #[test]
    fn test_completion_failure_with_repeat_off_stops_at_end_without_wrapping() {
        let mut pc = PlaylistController::new(RepeatMode::Off, false, 7);
        pc.load_tracks(2);
        pc.current_track = Some(0);

        // Track 0 completes -> tries track 1
        assert_eq!(pc.next_on_completion(), CompletionAction::Play(1));
        // Track 1 fails to decode -> must stop instead of wrapping back to track 0
        assert_eq!(pc.handle_completion_failure(1), CompletionAction::Stop);
    }

    #[test]
    fn test_completion_failure_with_repeat_off_shuffle_stops_at_end() {
        let mut pc = PlaylistController::new(RepeatMode::Off, true, 99);
        pc.load_tracks(3);
        let first = pc.shuffle_indices[0];
        let second = pc.shuffle_indices[1];
        let third = pc.shuffle_indices[2];
        pc.current_track = Some(first);

        assert_eq!(pc.next_on_completion(), CompletionAction::Play(second));
        assert_eq!(
            pc.handle_completion_failure(second),
            CompletionAction::Play(third)
        );
        assert_eq!(pc.handle_completion_failure(third), CompletionAction::Stop);
    }
}
