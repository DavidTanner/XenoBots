//! The campaign's rules (T-046, `docs/screens.md`, "Campaign rules"): the check after
//! every change, a scenario played, won or lost, the training menu, and the developer
//! keys. A mission's end and the developer keys Ctrl+W and Ctrl+L share the won and
//! lost routines, as in the original. The values (bonus, limits, chances) are the
//! user's, read by [`xeno_formats::campaign`].
//!
//! The alert rise and the calendar draw from the host's random word, reproduced in
//! [`Random`]. The original seeds it from the timer chip; here it starts from a fixed
//! seed so that a campaign replays the same, until the sim's own seeded source (T-050)
//! takes it over.

use xeno_formats::campaign::Campaign as Rules;
use xeno_formats::ending::Numbers;

use crate::maproom::{Campaign, IN_PLAY, LOCKED, WON};

/// The highest open `state`; the alert rise leaves it alone.
const TOP_ALERT: u16 = 6;
/// The result flag while the training menu is open; the rules then change nothing.
pub const TRAINING: u8 = 3;
/// The port's seed for a new campaign's random word.
const SEED: u32 = 0x5845_4e4f;

/// The host's random word (`sim_random_word`, `1000:0283`): four 16-bit shift
/// registers used in turn; each step shifts one right and, if a 1 fell out, XORs its
/// high byte with 0xb4. A call returns the new word less one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Random {
    words: [u16; 4],
    next: usize,
}

impl Default for Random {
    fn default() -> Self {
        Random::seeded(SEED)
    }
}

impl Random {
    /// Four odd words from `seed` (the original makes its timer reads odd too).
    pub fn seeded(seed: u32) -> Random {
        let mut state = seed;
        let words = std::array::from_fn(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 16) as u16 | 1
        });
        Random { words, next: 0 }
    }

    /// The next word, 0 to 0xfffe.
    pub fn word(&mut self) -> u16 {
        let at = self.next;
        self.next = (at + 1) % self.words.len();
        let word = &mut self.words[at];
        let carry = *word & 1 != 0;
        *word >>= 1;
        if carry {
            *word ^= 0xb400;
        }
        word.wrapping_sub(1)
    }

    /// Whether the next word is below `chance` (`sim_random_below`).
    pub fn below(&mut self, chance: u16) -> bool {
        self.word() < chance
    }
}

/// What a check finds open: scenarios within the progress and not won.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Open {
    /// The lowest unlock level among them (0xffff with none), and the highest less it.
    pub lowest: u16,
    pub spread: u16,
    pub count: u16,
}

impl Campaign {
    /// `scen_update_campaign_progress`: the campaign is won once every scenario has
    /// been; otherwise a scenario above the progress is locked (8, whatever its
    /// `state`), one at 8 within it opens (0), and the open ones are counted.
    pub fn check(&mut self) {
        if usize::from(self.wins) == self.table.scenarios.len() {
            self.won = true;
            return;
        }
        let (mut lowest, mut highest, mut count) = (u16::MAX, 0, 0);
        for scenario in &mut self.table.scenarios {
            if scenario.unlock_level > self.progress {
                scenario.state = LOCKED;
                continue;
            }
            if scenario.state == WON {
                continue;
            }
            if scenario.state == LOCKED {
                scenario.state = 0;
            }
            lowest = lowest.min(scenario.unlock_level);
            highest = highest.max(scenario.unlock_level);
            count += 1;
        }
        let spread = highest.wrapping_sub(lowest);
        self.open = Open {
            lowest,
            spread,
            count,
        };
    }

    /// The first scenario within the progress and not won (`ui_map_room_select_first`).
    pub fn first_selectable(&self) -> Option<usize> {
        (0..self.table.scenarios.len()).find(|&i| self.selectable(i))
    }

    /// A mission starts (`ui_map_room_run_mission`): no result yet, and the selected
    /// scenario is in play.
    pub fn run_mission(&mut self, selection: Option<usize>) {
        self.result = 0;
        if let Some(scenario) = selection.and_then(|i| self.table.scenarios.get_mut(i)) {
            scenario.state = IN_PLAY;
        }
    }

    /// The training menu opens (`ui_map_room_open_training`): the result is 3 and the
    /// loaded scenario forgotten; the caller clears the selection.
    pub fn open_training(&mut self) {
        self.result = TRAINING;
        self.loaded = None;
    }

    /// The selected scenario is won (`scen_win_selected_scenario`) with the mission's
    /// kills, losses and points. Returns the numbers the `WIN` caption shows (the points
    /// with the bonus) and the new selection, or `None` when nothing changes (no
    /// selection, or the training menu's result).
    pub fn win(
        &mut self,
        selection: Option<usize>,
        mission: Numbers,
        rules: &Rules,
    ) -> Option<(Numbers, Option<usize>)> {
        let i = selection.filter(|&i| i < self.table.scenarios.len())?;
        if self.result == TRAINING {
            return None;
        }
        self.table.scenarios[i].state = WON;
        self.wins = self.wins.wrapping_add(1);
        self.result = 1;
        let points = mission.points.wrapping_add(rules.win_bonus);
        self.score_mission(mission, points);
        self.check();
        if self.open.count < u16::from(rules.open_limit) {
            self.progress += 1;
            self.check();
        }
        if self.open.spread != 0 {
            for i in 0..self.table.scenarios.len() {
                let scenario = &self.table.scenarios[i];
                let (level, state) = (scenario.unlock_level, scenario.state);
                if level > self.progress || state == WON || state == TOP_ALERT {
                    continue;
                }
                let chance = rules.alert_chance(level.wrapping_sub(self.open.lowest));
                if self.random.below(chance) {
                    // A scenario in play is not spared, so it can pass 12 (T-046).
                    self.table.scenarios[i].state = state + 2;
                }
            }
        }
        let selection = self.first_selectable();
        self.advance_day();
        Some((Numbers { points, ..mission }, selection))
    }

    /// The loaded scenario is lost (`scen_lose_loaded_scenario`): it is forgotten, so
    /// playing it loads it again, and its `state` stays 12. `None` with the training
    /// menu's result.
    pub fn lose(&mut self, mission: Numbers) -> Option<Numbers> {
        if self.result == TRAINING {
            return None;
        }
        self.check();
        self.loaded = None;
        self.result = 2;
        self.losses = self.losses.wrapping_add(1);
        self.score_mission(mission, mission.points);
        self.advance_day();
        Some(mission)
    }

    /// `=` and `-`: the progress up to the user's cap, or down to 0, then the check.
    pub fn raise_progress(&mut self, rules: &Rules) {
        if self.progress != u16::from(rules.max_progress) {
            self.progress += 1;
        }
        self.check();
    }

    pub fn lower_progress(&mut self) {
        self.progress = self.progress.saturating_sub(1);
        self.check();
    }

    /// `scen_score_mission_result` and its caller: the totals and the score.
    fn score_mission(&mut self, mission: Numbers, points: u16) {
        self.kills = self.kills.wrapping_add(mission.kills.into());
        self.robots_lost = self.robots_lost.wrapping_add(mission.losses.into());
        self.score = self.score.wrapping_add(points);
    }

    /// `scen_advance_campaign_day`: 0 to 3 days pass.
    fn advance_day(&mut self) {
        self.day = self.day.wrapping_add((self.random.word() & 3) as u8);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::maproom::tests::test_campaign;
    use xeno_formats::maproom::Rect;

    pub(crate) fn rules() -> Rules {
        Rules {
            developer_unlock: 1,
            editor_unlock: 2,
            win_bonus: 1000,
            open_limit: 3,
            max_progress: 16,
            alert_chances: vec![0x8000, 0x3000, 0x1000, 0],
            hover_words: Default::default(),
            won_box: Rect {
                top: 60,
                left: 90,
                bottom: 110,
                right: 230,
            },
            won_message: b"W".to_vec(),
            prompt: b"P".to_vec(),
            prompt_won: b"Q".to_vec(),
        }
    }

    fn numbers(kills: u8, losses: u8, points: u16) -> Numbers {
        Numbers {
            kills,
            losses,
            points,
        }
    }

    #[test]
    fn the_random_word_is_the_hosts_shift_register() {
        let mut random = Random {
            words: [1, 2, 3, 4],
            next: 0,
        };
        // 1 shifts out a 1: 0 ^ 0xb400; 2 shifts to 1; 3 to 1 ^ 0xb400; 4 to 2.
        let words: Vec<_> = (0..5).map(|_| random.word()).collect();
        assert_eq!(words, [0xb3ff, 0, 0xb400, 1, 0x59ff]);
        assert_eq!(Random::seeded(7), Random::seeded(7));
        assert!(Random::default().words.iter().all(|w| w & 1 == 1));
    }

    #[test]
    fn a_win_scores_raises_progress_and_selects_the_first() {
        let rules = rules();
        let mut campaign = test_campaign();
        // Open: 0, 3, 4 (level 1); 1 won, 2 locked (level 2).
        campaign.check();
        assert_eq!(campaign.open.count, 3);
        campaign.run_mission(Some(0));
        assert_eq!(
            (campaign.result, campaign.table.scenarios[0].state),
            (0, IN_PLAY)
        );
        let (shown, selection) = campaign.win(Some(0), numbers(3, 1, 125), &rules).unwrap();
        assert_eq!(shown.points, 1125);
        assert_eq!(
            (campaign.score, campaign.wins, campaign.result),
            (1125, 1, 1)
        );
        assert_eq!((campaign.kills, campaign.robots_lost), (3, 1));
        // Two open, under three: the progress rises and opens scenario 2.
        assert_eq!(campaign.progress, 2);
        assert_eq!(campaign.table.scenarios[0].state, WON);
        assert!(campaign.table.scenarios[2].state != LOCKED);
        assert_eq!(selection, Some(2));
    }

    #[test]
    fn alerts_rise_by_chance_and_a_scenario_in_play_can_pass_12() {
        let mut rules = rules();
        rules.alert_chances = vec![0xffff, 0xffff, 0xffff];
        rules.open_limit = 0;
        let mut campaign = test_campaign();
        campaign.progress = 2;
        campaign.table.scenarios[3].state = IN_PLAY;
        campaign.table.scenarios[4].state = TOP_ALERT;
        campaign.check();
        campaign.win(Some(0), numbers(0, 0, 0), &rules).unwrap();
        let states: Vec<_> = campaign.table.scenarios.iter().map(|s| s.state).collect();
        // 0 won, 1 already won, 2 raised, 3 in play raised to 14, 4 at the top.
        assert_eq!(states, [WON, WON, 2, IN_PLAY + 2, TOP_ALERT]);
        // With every open scenario at one level, nothing rises.
        let mut flat = test_campaign();
        flat.check();
        flat.win(Some(0), numbers(0, 0, 0), &rules).unwrap();
        assert!(flat.table.scenarios[3].state == 0 && flat.table.scenarios[4].state == 0);
    }

    #[test]
    fn a_loss_forgets_the_loaded_scenario_and_training_changes_nothing() {
        let rules = rules();
        let mut campaign = test_campaign();
        campaign.loaded = Some(3);
        campaign.run_mission(Some(3));
        assert_eq!(campaign.lose(numbers(0, 2, 0)), Some(numbers(0, 2, 0)));
        assert_eq!(
            (campaign.loaded, campaign.result, campaign.losses),
            (None, 2, 1)
        );
        assert_eq!(campaign.table.scenarios[3].state, IN_PLAY);
        campaign.open_training();
        let before = campaign.clone();
        assert_eq!(campaign.lose(numbers(1, 1, 1)), None);
        assert_eq!(campaign.win(Some(0), numbers(1, 1, 1), &rules), None);
        assert_eq!(campaign, before);
    }

    #[test]
    fn the_last_win_wins_the_campaign_and_progress_keys_lock_and_reopen() {
        let rules = rules();
        let mut campaign = test_campaign();
        campaign.lower_progress();
        assert_eq!(campaign.progress, 0);
        assert!(campaign.table.scenarios.iter().all(|s| s.state == LOCKED));
        campaign.raise_progress(&rules);
        // The won scenario was locked, and opens at 0: the original's quirk.
        assert_eq!(campaign.table.scenarios[1].state, 0);
        campaign.wins = 4;
        campaign.result = 2;
        campaign.win(Some(0), numbers(0, 0, 0), &rules).unwrap();
        assert!(campaign.won);
        campaign.progress = 16;
        campaign.raise_progress(&rules);
        assert_eq!(campaign.progress, 16);
    }
}
