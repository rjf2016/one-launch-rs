// What a launch came to: what was sent, whether the launcher was seen starting the game, and what is left for the user to do in the launcher's window.
pub struct Launched {
    pub command: String,
    pub game_seen: bool,
    pub hand_off: Option<String>,
}

impl Launched {
    pub const fn sent(command: String) -> Self {
        Self {
            command,
            game_seen: false,
            hand_off: None,
        }
    }

    pub const fn started(command: String) -> Self {
        Self {
            command,
            game_seen: true,
            hand_off: None,
        }
    }
}
