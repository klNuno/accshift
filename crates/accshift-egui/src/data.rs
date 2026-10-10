//! The frozen `demo` scenario of `src/lib/mock/scenarios.ts`, ported as is so
//! the egui captures line up with the README clips frame for frame.

pub const NOW: u64 = 1_791_000_000;
const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

/// The 21 demo faces, shared with the web mock (`src/lib/mock/avatars`).
pub const AVATARS: [&[u8]; 21] = [
    include_bytes!("../../../src/lib/mock/avatars/1.svg"),
    include_bytes!("../../../src/lib/mock/avatars/2.svg"),
    include_bytes!("../../../src/lib/mock/avatars/3.svg"),
    include_bytes!("../../../src/lib/mock/avatars/4.svg"),
    include_bytes!("../../../src/lib/mock/avatars/5.svg"),
    include_bytes!("../../../src/lib/mock/avatars/6.svg"),
    include_bytes!("../../../src/lib/mock/avatars/7.svg"),
    include_bytes!("../../../src/lib/mock/avatars/8.svg"),
    include_bytes!("../../../src/lib/mock/avatars/9.svg"),
    include_bytes!("../../../src/lib/mock/avatars/10.svg"),
    include_bytes!("../../../src/lib/mock/avatars/11.svg"),
    include_bytes!("../../../src/lib/mock/avatars/12.svg"),
    include_bytes!("../../../src/lib/mock/avatars/13.svg"),
    include_bytes!("../../../src/lib/mock/avatars/14.svg"),
    include_bytes!("../../../src/lib/mock/avatars/15.svg"),
    include_bytes!("../../../src/lib/mock/avatars/16.svg"),
    include_bytes!("../../../src/lib/mock/avatars/17.svg"),
    include_bytes!("../../../src/lib/mock/avatars/18.svg"),
    include_bytes!("../../../src/lib/mock/avatars/19.svg"),
    include_bytes!("../../../src/lib/mock/avatars/20.svg"),
    include_bytes!("../../../src/lib/mock/avatars/21.svg"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Steam,
    Riot,
    Roblox,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Steam, Platform::Riot, Platform::Roblox];

    pub fn name(self) -> &'static str {
        match self {
            Platform::Steam => "Steam",
            Platform::Riot => "Riot Games",
            Platform::Roblox => "Roblox",
        }
    }

    /// `PLATFORM_CHROME[id].accent` in `src/lib/platforms/registry.ts`.
    pub fn accent(self) -> u32 {
        match self {
            Platform::Steam => 0x2563eb,
            Platform::Riot => 0xef4444,
            Platform::Roblox => 0xe1242a,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub last_login: Option<u64>,
    /// Index into [`AVATARS`]; `None` draws the gradient and initials.
    pub avatar: Option<usize>,
    pub color: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Account(usize),
    Folder {
        id: &'static str,
        name: &'static str,
    },
}

pub struct Dataset {
    pub steam: Vec<Account>,
    pub riot: Vec<Account>,
    pub roblox: Vec<Account>,
    pub steam_root: Vec<Item>,
    pub smurfs: Vec<Item>,
    pub active_steam: String,
    pub active_riot: String,
    pub active_roblox: String,
}

impl Dataset {
    pub fn accounts(&self, platform: Platform) -> &[Account] {
        match platform {
            Platform::Steam => &self.steam,
            Platform::Riot => &self.riot,
            Platform::Roblox => &self.roblox,
        }
    }

    pub fn active(&self, platform: Platform) -> &str {
        match platform {
            Platform::Steam => &self.active_steam,
            Platform::Riot => &self.active_riot,
            Platform::Roblox => &self.active_roblox,
        }
    }

    pub fn set_active(&mut self, platform: Platform, id: String) {
        match platform {
            Platform::Steam => self.active_steam = id,
            Platform::Riot => self.active_riot = id,
            Platform::Roblox => self.active_roblox = id,
        }
    }
}

fn account(id: &str, name: &str, last_login: Option<u64>, avatar: Option<usize>) -> Account {
    Account {
        id: id.to_string(),
        name: name.to_string(),
        last_login,
        avatar,
        color: None,
    }
}

pub fn demo() -> Dataset {
    let steam_names: [(&str, Option<u64>); 17] = [
        ("main", Some(NOW - 2 * HOUR)),
        ("bro's account", Some(NOW - 3 * DAY)),
        ("alt", Some(NOW - 9 * DAY)),
        ("trading", Some(NOW - 21 * DAY)),
        ("faceit", Some(NOW - HOUR)),
        ("ranked grind", Some(NOW - 5 * DAY)),
        ("chill", Some(NOW - 14 * DAY)),
        ("lan party", Some(NOW - 40 * DAY)),
        ("streams", Some(NOW - 4 * HOUR)),
        ("esea", Some(NOW - 8 * DAY)),
        ("tournaments", Some(NOW - 60 * DAY)),
        ("family", Some(NOW - 2 * DAY)),
        ("practice", Some(NOW - 6 * HOUR)),
        ("smurf 1", Some(NOW - 12 * DAY)),
        ("smurf 2", Some(NOW - 27 * DAY)),
        ("smurf 3", Some(NOW - 33 * DAY)),
        ("smurf 4", None),
    ];
    let mut steam: Vec<Account> = steam_names
        .iter()
        .enumerate()
        .map(|(i, (name, last))| {
            account(
                &format!("765611980000000{:02}", i + 1),
                name,
                *last,
                Some(i),
            )
        })
        .collect();
    // "client.account-card-colors" of the scenario.
    steam[0].color = Some(0x8b5cf6);
    steam[4].color = Some(0xf97316);
    steam[8].color = Some(0xec4899);

    let riot = vec![
        account("riot-1", "main", Some(NOW - HOUR), None),
        account("riot-2", "smurf", Some(NOW - 6 * DAY), None),
        account("riot-3", "duo account", Some(NOW - 19 * DAY), None),
    ];
    let roblox = vec![
        account("1000000001", "builder", Some(NOW - 2 * HOUR), Some(17)),
        account("1000000002", "obby runs", Some(NOW - 3 * DAY), Some(18)),
        account("1000000003", "tycoon alt", Some(NOW - 11 * DAY), Some(19)),
        account("1000000004", "trading", Some(NOW - 25 * DAY), Some(20)),
    ];

    // The web grid puts folders first, then accounts in saved order.
    let mut steam_root = vec![Item::Folder {
        id: "demo-folder-smurfs",
        name: "Smurfs",
    }];
    steam_root.extend((0..13).map(Item::Account));
    let smurfs = (13..17).map(Item::Account).collect();

    Dataset {
        active_steam: steam[0].id.clone(),
        active_riot: riot[0].id.clone(),
        active_roblox: roblox[0].id.clone(),
        steam,
        riot,
        roblox,
        steam_root,
        smurfs,
    }
}

/// "2h ago", "3d ago", in the spirit of `formatRelativeTimeCompact`.
pub fn relative_time(ts: Option<u64>) -> String {
    let Some(ts) = ts else {
        return "Never".to_string();
    };
    let diff = NOW.saturating_sub(ts);
    if diff < HOUR {
        format!("{}m ago", diff / 60)
    } else if diff < DAY {
        format!("{}h ago", diff / HOUR)
    } else if diff < 30 * DAY {
        format!("{}d ago", diff / DAY)
    } else {
        format!("{}mo ago", diff / (30 * DAY))
    }
}

/// Initials as `getAvatarInitials` builds them.
pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.as_slice() {
        [] => "?".to_string(),
        [one] => one.chars().take(1).collect::<String>().to_uppercase(),
        [a, b, ..] => format!(
            "{}{}",
            a.chars().next().unwrap_or_default(),
            b.chars().next().unwrap_or_default()
        )
        .to_uppercase(),
    }
}
