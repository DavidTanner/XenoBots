//! The player's save game, `<NAME>.LOG` (T-057, `docs/formats/usg.md`, "Save files"):
//! the loaded scenario's chunks with an `SDEF` chunk after `SHDR` holding the campaign.
//! The log-in reads it ([`load`]); the map room's N and Q write it ([`write`]) once a
//! game is loaded.
//!
//! shortcut: the chunks written are the loaded scenario as it was loaded (and edited),
//! not the running sim's world or clock, so a mission resumed from a save starts over
//! (T-116 writes the live world).

use std::path::{Path, PathBuf};

use xeno_formats::FormatError;
use xeno_formats::usg::{self, SaveState, Scenario};

use crate::maproom::Campaign;

/// The save's file name: the original fills its 8.3 template `        .LOG` with the
/// name, and DOS drops the padding spaces.
pub fn file_name(name: &str) -> String {
    format!("{name}.LOG")
}

/// `<NAME>.LOG` in `dir`, whatever the case of its name.
pub fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = file_name(name).to_lowercase();
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.to_lowercase() == want)
        })
}

/// A record's position from a saved id (`SaveState::NONE`: none).
fn index(id: u16, campaign: &Campaign) -> Option<usize> {
    let i = usize::from(id / 2);
    (id != SaveState::NONE && i < campaign.table.scenarios.len()).then_some(i)
}

/// A record's id, as the save stores it.
fn id(i: Option<usize>, campaign: &Campaign) -> u16 {
    i.and_then(|i| campaign.table.scenarios.get(i))
        .map_or(SaveState::NONE, |s| s.id)
}

/// Reads a save into `campaign` as `scen_read_sdef` and the log-in do: the table, the
/// counters and the result, the calendar's day from `SHDR`, then the campaign check.
/// Returns the map room's selection and the loaded scenario. On an error `campaign` is
/// left as it was.
pub fn load(bytes: &[u8], campaign: &mut Campaign) -> Result<(Option<usize>, Scenario), String> {
    let scenario = usg::parse(bytes).map_err(|e| e.to_string())?;
    let save = scenario
        .save
        .as_ref()
        .ok_or("no SDEF chunk: a scenario, not a save")?;
    campaign.table = save.scenarios().map_err(|e| e.to_string())?;
    campaign.progress = save.progress;
    campaign.score = save.score;
    campaign.wins = save.wins;
    campaign.losses = save.losses;
    campaign.kills = save.kills;
    campaign.robots_lost = save.robots_lost;
    campaign.result = save.result;
    campaign.won = save.campaign_won != 0;
    campaign.loaded = index(save.loaded, campaign);
    campaign.day = scenario.header.start.day;
    campaign.game_loaded = true;
    campaign.check();
    Ok((index(save.selected, campaign), scenario))
}

/// Encodes the save `file_write_save_game` writes: `scenario`'s chunks with the
/// campaign's day in its clock and the campaign as `SDEF`.
pub fn write(
    campaign: &Campaign,
    selection: Option<usize>,
    scenario: &Scenario,
) -> Result<Vec<u8>, FormatError> {
    let mut save = SaveState {
        selected: id(selection, campaign),
        loaded: id(campaign.loaded, campaign),
        progress: campaign.progress,
        score: campaign.score,
        wins: campaign.wins,
        losses: campaign.losses,
        kills: campaign.kills,
        robots_lost: campaign.robots_lost,
        unknown_10: 0,
        result: campaign.result,
        campaign_won: campaign.won.into(),
        unknown_16: [0; 8],
        table: Vec::new(),
    };
    save.set_scenarios(&campaign.table);
    let mut scenario = scenario.clone();
    // shortcut: the month stays the scenario's; the port keeps no campaign month yet.
    scenario.header.start.day = campaign.day;
    scenario.save = Some(save);
    usg::write(&scenario)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maproom::{IN_PLAY, tests::test_campaign};
    use xeno_formats::fixtures::synthetic_usg;
    use xeno_sim::Random;

    #[test]
    fn a_saved_campaign_loads_back_the_same() {
        let mut campaign = test_campaign();
        campaign.table.scenarios[3].state = IN_PLAY;
        (campaign.progress, campaign.score, campaign.wins) = (2, 1500, 1);
        (campaign.losses, campaign.kills, campaign.robots_lost) = (2, 40, 9);
        (campaign.result, campaign.loaded, campaign.day) = (0, Some(3), 5);
        campaign.game_loaded = true;
        campaign.check();
        let scenario = usg::parse(&synthetic_usg()).unwrap();
        let bytes = write(&campaign, Some(3), &scenario).unwrap();

        let mut back = Campaign::new(test_campaign().table, Random::seeded(1));
        let (selection, loaded) = load(&bytes, &mut back).unwrap();
        assert_eq!(selection, Some(3));
        assert_eq!(loaded.objects, scenario.objects);
        assert_eq!(loaded.header.start.day, 5);
        assert_eq!(back, campaign);
        // Written again, the same bytes.
        assert_eq!(write(&back, selection, &loaded).unwrap(), bytes);
    }

    #[test]
    fn a_scenario_without_sdef_is_not_a_save() {
        let mut campaign = test_campaign();
        let before = campaign.clone();
        let err = load(&synthetic_usg(), &mut campaign).unwrap_err();
        assert!(err.contains("SDEF"), "{err}");
        assert_eq!(campaign, before);
    }

    #[test]
    fn the_save_is_found_whatever_its_case() {
        let dir = std::env::temp_dir().join(format!("xeno-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ace.log"), b"").unwrap();
        assert_eq!(find(&dir, "ACE"), Some(dir.join("ace.log")));
        assert_eq!(find(&dir, "AC"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
