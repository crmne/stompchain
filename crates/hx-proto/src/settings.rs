//! The device's global settings, by name.
//!
//! A setting is a numbered object - `{118: id, 119: value}` - and the numbers
//! are all the device offers: nothing in HX Edit's shipped data names them. So
//! the names here were earned by watching HX Edit write them, one control at a
//! time, each under its own mark in a USB capture (`tools/hxsniff/capture.sh
//! globals`). What is listed is what was seen written and can be shown honestly;
//! the rest of the 154 the pedal answers for are reachable only from its own
//! front-panel menu, so no capture will ever name them and guessing would be
//! worse than leaving them out.
//!
//! These are *global*: they belong to the pedal, not to a preset, and changing
//! one changes how every preset behaves.

/// What kind of value a setting holds, and how to show it.
pub enum Kind {
    /// A switch, with the labels for off and on in that order.
    Switch(&'static str, &'static str),
    /// One of a fixed list, indexed from zero.
    Choice(&'static [&'static str]),
    /// A number, with its range and the unit to show after it.
    Number {
        min: f32,
        max: f32,
        unit: &'static str,
    },
}

/// One named global setting.
pub struct Setting {
    /// The object id it is read and written by.
    pub id: i64,
    pub name: &'static str,
    /// Which panel it belongs under, so the UI can group without a second table.
    pub group: &'static str,
    pub kind: Kind,
}

/// What the three assignable footswitches can be made to do.
///
/// One numbering shared by FS3, FS4 and FS5; FS3 simply offers fewer of it.
const FOOTSWITCH: &[&str] = &[
    "Tap/Tuner",
    "Stomp",
    "Bank Up",
    "Bank Down",
    "Preset Up",
    "Preset Down",
    "Snapshot Up",
    "Snapshot Down",
    "FS Mode >",
    "< FS Mode",
    "All Bypass",
    "Toggle EXP",
];

/// The settings this program knows the names of.
pub const SETTINGS: &[Setting] = &[
    Setting {
        id: 16,
        name: "Tempo",
        group: "Tempo",
        kind: Kind::Number { min: 40.0, max: 240.0, unit: " BPM" },
    },
    Setting {
        id: 14,
        name: "Tempo follows",
        group: "Tempo",
        kind: Kind::Choice(&["Per Snapshot", "Per Preset", "Global", "Host Sync"]),
    },
    Setting {
        id: 27,
        name: "Preset numbering",
        group: "Display",
        kind: Kind::Switch("01A - 42C", "000 - 125"),
    },
    Setting {
        id: 95,
        name: "EXP/FS Tip",
        group: "Pedal jacks",
        kind: Kind::Switch("EXP 1", "FS4"),
    },
    Setting {
        id: 96,
        name: "EXP/FS Ring",
        group: "Pedal jacks",
        kind: Kind::Switch("EXP 2", "FS5"),
    },
    Setting { id: 97, name: "FS3", group: "Footswitches", kind: Kind::Choice(FOOTSWITCH) },
    Setting { id: 98, name: "FS4", group: "Footswitches", kind: Kind::Choice(FOOTSWITCH) },
    Setting { id: 99, name: "FS5", group: "Footswitches", kind: Kind::Choice(FOOTSWITCH) },
    Setting {
        id: 203,
        name: "Global EQ",
        group: "Global EQ",
        kind: Kind::Switch("Off", "On"),
    },
    Setting {
        id: 199,
        name: "Low Cut",
        // 19.9 is the device's "off" - below the band rather than in it, which
        // is why the range starts under 20 Hz rather than at it.
        group: "Global EQ",
        kind: Kind::Number { min: 19.9, max: 500.0, unit: " Hz" },
    },
    Setting {
        id: 190,
        name: "Low Freq",
        group: "Global EQ",
        kind: Kind::Number { min: 20.0, max: 500.0, unit: " Hz" },
    },
    Setting {
        id: 191,
        name: "Low Q",
        group: "Global EQ",
        kind: Kind::Number { min: 0.1, max: 10.0, unit: "" },
    },
    Setting {
        id: 192,
        name: "Low Gain",
        group: "Global EQ",
        kind: Kind::Number { min: -12.0, max: 12.0, unit: " dB" },
    },
    Setting {
        id: 193,
        name: "Mid Freq",
        group: "Global EQ",
        kind: Kind::Number { min: 200.0, max: 5000.0, unit: " Hz" },
    },
    Setting {
        id: 194,
        name: "Mid Q",
        group: "Global EQ",
        kind: Kind::Number { min: 0.1, max: 10.0, unit: "" },
    },
    Setting {
        id: 195,
        name: "Mid Gain",
        group: "Global EQ",
        kind: Kind::Number { min: -12.0, max: 12.0, unit: " dB" },
    },
    Setting {
        id: 196,
        name: "High Freq",
        group: "Global EQ",
        kind: Kind::Number { min: 1000.0, max: 20000.0, unit: " Hz" },
    },
    Setting {
        id: 197,
        name: "High Q",
        group: "Global EQ",
        kind: Kind::Number { min: 0.1, max: 10.0, unit: "" },
    },
    Setting {
        id: 198,
        name: "High Gain",
        group: "Global EQ",
        kind: Kind::Number { min: -12.0, max: 12.0, unit: " dB" },
    },
    Setting {
        id: 200,
        name: "High Cut",
        // 20100 is the device's "off", just above the audible top.
        group: "Global EQ",
        kind: Kind::Number { min: 1000.0, max: 20100.0, unit: " Hz" },
    },
];

/// The groups, in the order they should be shown.
pub fn groups() -> Vec<&'static str> {
    let mut seen = Vec::new();
    for s in SETTINGS {
        if !seen.contains(&s.group) {
            seen.push(s.group);
        }
    }
    seen
}

/// Look one up by id.
pub fn setting(id: i64) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_is_listed_once_and_grouped() {
        let mut ids: Vec<i64> = SETTINGS.iter().map(|s| s.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "an id is listed twice");

        // The Global EQ is the big group: a bypass, two cuts and three peaks of
        // frequency, Q and gain - the eleven values op76 returns as one array.
        let eq = SETTINGS.iter().filter(|s| s.group == "Global EQ").count();
        assert_eq!(eq, 12, "the bypass plus the eleven coefficients");
        assert_eq!(groups().first(), Some(&"Tempo"));
    }

    #[test]
    fn the_footswitch_choices_match_the_devices_own_numbering() {
        let Kind::Choice(list) = &setting(97).unwrap().kind else {
            panic!("FS3 is a choice");
        };
        assert_eq!(list[0], "Tap/Tuner");
        assert_eq!(list[11], "Toggle EXP");
        // FS3, FS4 and FS5 share one numbering, which is why they share a list.
        for id in [98, 99] {
            let Kind::Choice(other) = &setting(id).unwrap().kind else {
                panic!("a choice");
            };
            assert_eq!(list.len(), other.len());
        }
    }
}
