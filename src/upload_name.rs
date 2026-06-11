//! Templated upload names.
//!
//! Replay files are uploaded with a multipart filename, which most destinations
//! surface as the replay's display name. By default that filename was just the
//! match GUID; this module renders a friendlier name from match metadata using a
//! small `{PLACEHOLDER}` template (see [`BehaviorConfig::upload_name_template`]).
//!
//! [`BehaviorConfig::upload_name_template`]: crate::config::BehaviorConfig::upload_name_template

use chrono::{Datelike, Local, TimeZone, Timelike};

use crate::psynet::{MatchEntry, MatchPlayer};

/// The default template applied to uploads when the user has not configured one.
/// Produces names like `2024-01-15.14.30 SaltySphinx Ranked Doubles Win`.
pub const DEFAULT_TEMPLATE: &str = "{YEAR}-{MONTH}-{DAY}.{HOUR}.{MIN} {PLAYER} {MODE} {WINLOSS}";

/// Renders an upload filename from `template` using the metadata in `entry`,
/// taken from the perspective of the synced account identified by
/// `account_player_id` (a PsyNet `Platform|id|split` string). `fallback_name` is
/// used for `{PLAYER}` when that account is not found among the match players.
///
/// Returns `None` when `template` is empty/blank or renders to nothing, so the
/// caller can fall back to its legacy match-id filename. The returned name is
/// sanitized for use as a filename and always ends in `.replay`.
pub fn render_upload_name(
    template: &str,
    entry: &MatchEntry,
    account_player_id: &str,
    fallback_name: &str,
) -> Option<String> {
    render_upload_name_in(template, entry, account_player_id, fallback_name, &Local)
}

/// [`render_upload_name`] with an explicit timezone, so the date placeholders
/// can be rendered deterministically (production uses the machine's local time).
fn render_upload_name_in<Tz: TimeZone>(
    template: &str,
    entry: &MatchEntry,
    account_player_id: &str,
    fallback_name: &str,
    tz: &Tz,
) -> Option<String> {
    if template.trim().is_empty() {
        return None;
    }
    let fields = Fields::from_match(entry, account_player_id, fallback_name, tz)?;
    finalize(&substitute(template, &fields))
}

/// Resolved placeholder values for a single match/account pair.
struct Fields {
    year: String,
    month: String,
    day: String,
    hour: String,
    minute: String,
    second: String,
    player: String,
    mode: String,
    map: String,
    winloss: String,
    score: String,
    match_id: String,
}

impl Fields {
    fn from_match<Tz: TimeZone>(
        entry: &MatchEntry,
        account_player_id: &str,
        fallback_name: &str,
        tz: &Tz,
    ) -> Option<Self> {
        let info = &entry.match_info;
        let datetime = tz.timestamp_opt(info.record_start_timestamp, 0).single()?;
        let player = resolve_player(&info.players, account_player_id);
        let player_name = player
            .map(|player| player.player_name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(fallback_name);
        let winloss = player
            .map(|player| match_result(player.last_team, info.team0_score, info.team1_score))
            .unwrap_or(MatchResult::Unknown);

        Some(Self {
            year: format!("{:04}", datetime.year()),
            month: format!("{:02}", datetime.month()),
            day: format!("{:02}", datetime.day()),
            hour: format!("{:02}", datetime.hour()),
            minute: format!("{:02}", datetime.minute()),
            second: format!("{:02}", datetime.second()),
            player: player_name.to_string(),
            mode: playlist_name(info.playlist),
            map: info.map_name.clone(),
            winloss: winloss.label().to_string(),
            score: format!("{}-{}", info.team0_score, info.team1_score),
            match_id: info.match_guid.clone(),
        })
    }
}

/// Replaces the supported `{PLACEHOLDER}` tokens in `template`. Unknown tokens
/// are left untouched.
fn substitute(template: &str, fields: &Fields) -> String {
    let replacements = [
        ("{YEAR}", fields.year.as_str()),
        ("{MONTH}", fields.month.as_str()),
        ("{DAY}", fields.day.as_str()),
        ("{HOUR}", fields.hour.as_str()),
        ("{MIN}", fields.minute.as_str()),
        ("{SEC}", fields.second.as_str()),
        ("{PLAYER}", fields.player.as_str()),
        ("{MODE}", fields.mode.as_str()),
        ("{MAP}", fields.map.as_str()),
        ("{WINLOSS}", fields.winloss.as_str()),
        ("{SCORE}", fields.score.as_str()),
        ("{MATCH_ID}", fields.match_id.as_str()),
    ];
    let mut rendered = template.to_string();
    for (token, value) in replacements {
        if rendered.contains(token) {
            rendered = rendered.replace(token, value);
        }
    }
    rendered
}

/// Collapses whitespace, strips characters that are unsafe in filenames, and
/// ensures the `.replay` extension. Returns `None` if nothing is left.
fn finalize(rendered: &str) -> Option<String> {
    let collapsed = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name: String = collapsed
        .chars()
        .filter(|ch| !is_unsafe_filename_char(*ch))
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    if !name.to_ascii_lowercase().ends_with(".replay") {
        name.push_str(".replay");
    }
    Some(name)
}

fn is_unsafe_filename_char(ch: char) -> bool {
    ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchResult {
    Win,
    Loss,
    Draw,
    Unknown,
}

impl MatchResult {
    fn label(self) -> &'static str {
        match self {
            Self::Win => "Win",
            Self::Loss => "Loss",
            Self::Draw => "Draw",
            Self::Unknown => "",
        }
    }
}

/// Determines the result for a player on `team` (0 or 1) from the team scores.
fn match_result(team: i64, team0_score: i64, team1_score: i64) -> MatchResult {
    let (ours, theirs) = match team {
        0 => (team0_score, team1_score),
        1 => (team1_score, team0_score),
        _ => return MatchResult::Unknown,
    };
    match ours.cmp(&theirs) {
        std::cmp::Ordering::Greater => MatchResult::Win,
        std::cmp::Ordering::Less => MatchResult::Loss,
        std::cmp::Ordering::Equal => MatchResult::Draw,
    }
}

/// Finds the synced account among the match players. Matches on the full PsyNet
/// PlayerID first, then falls back to the platform-specific id component (the
/// middle `Platform|id|split` field), which is stable across split-screen slots.
fn resolve_player<'a>(
    players: &'a [MatchPlayer],
    account_player_id: &str,
) -> Option<&'a MatchPlayer> {
    if let Some(player) = players
        .iter()
        .find(|player| player.player_id == account_player_id)
    {
        return Some(player);
    }
    let account_id = player_id_component(account_player_id)?;
    players
        .iter()
        .find(|player| player_id_component(&player.player_id) == Some(account_id))
}

/// Extracts the platform-specific id (middle component) of a `Platform|id|split`
/// PsyNet PlayerID.
fn player_id_component(player_id: &str) -> Option<&str> {
    let component = player_id.split('|').nth(1)?;
    (!component.is_empty()).then_some(component)
}

/// Best-effort mapping of PsyNet playlist ids to display names. Unknown ids fall
/// back to `Playlist <id>` so newly added or niche modes still render usefully.
fn playlist_name(playlist: i64) -> String {
    let name = match playlist {
        1 => "Casual Duel",
        2 => "Casual Doubles",
        3 => "Casual Standard",
        4 => "Casual Chaos",
        6 => "Private",
        10 => "Ranked Duel",
        11 => "Ranked Doubles",
        12 => "Ranked Solo Standard",
        13 => "Ranked Standard",
        27 => "Hoops",
        28 => "Rumble",
        29 => "Dropshot",
        30 => "Snow Day",
        34 => "Tournament",
        _ => return format!("Playlist {playlist}"),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psynet::Match;
    use chrono::{FixedOffset, Utc};

    fn match_entry() -> MatchEntry {
        MatchEntry {
            replay_url: "https://example.com/replay".to_string(),
            match_info: Match {
                match_guid: "MATCH-GUID".to_string(),
                // 2024-01-15 14:30:05 UTC
                record_start_timestamp: 1_705_329_005,
                map_name: "DFH Stadium".to_string(),
                playlist: 11,
                team0_score: 3,
                team1_score: 1,
                players: vec![
                    MatchPlayer {
                        player_id: "Epic|me|0".to_string(),
                        player_name: "SaltySphinx".to_string(),
                        last_team: 1,
                        ..MatchPlayer::default()
                    },
                    MatchPlayer {
                        player_id: "Steam|rival|0".to_string(),
                        player_name: "Rival".to_string(),
                        last_team: 0,
                        ..MatchPlayer::default()
                    },
                ],
            },
        }
    }

    fn fields(entry: &MatchEntry, account_player_id: &str, fallback: &str) -> Fields {
        Fields::from_match(entry, account_player_id, fallback, &Utc).unwrap()
    }

    #[test]
    fn default_template_renders_date_player_mode_and_result() {
        // last_team 1 with scores 3-1 means our (team1) side lost.
        let rendered = substitute(
            DEFAULT_TEMPLATE,
            &fields(&match_entry(), "Epic|me|0", "Primary"),
        );
        assert_eq!(rendered, "2024-01-15.14.30 SaltySphinx Ranked Doubles Loss");
    }

    #[test]
    fn winner_perspective_reports_win() {
        // The rival is on team 0, which won 3-1.
        let rendered = substitute(
            "{WINLOSS} {PLAYER}",
            &fields(&match_entry(), "Steam|rival|0", "x"),
        );
        assert_eq!(rendered, "Win Rival");
    }

    #[test]
    fn resolves_player_by_id_component_when_full_id_differs() {
        // Split-screen / differing trailing component still matches on the id.
        let rendered = substitute("{PLAYER}", &fields(&match_entry(), "Epic|me|1", "Primary"));
        assert_eq!(rendered, "SaltySphinx");
    }

    #[test]
    fn unknown_account_uses_fallback_name_and_blank_result() {
        let rendered = substitute(
            "{PLAYER}|{WINLOSS}",
            &fields(&match_entry(), "Epic|stranger|0", "Primary"),
        );
        assert_eq!(rendered, "Primary|");
    }

    #[test]
    fn finalize_collapses_blanks_strips_unsafe_chars_and_adds_extension() {
        // Blank {WINLOSS} leaves a trailing gap that should collapse away, and an
        // unsafe character in a player name is removed.
        let entry = {
            let mut entry = match_entry();
            entry.match_info.players[0].player_name = "Salty/Sphinx".to_string();
            entry.match_info.team0_score = 1;
            entry.match_info.team1_score = 1;
            entry
        };
        let name = render_upload_name_in(DEFAULT_TEMPLATE, &entry, "Epic|me|0", "Primary", &Utc);
        assert_eq!(
            name.as_deref(),
            Some("2024-01-15.14.30 SaltySphinx Ranked Doubles Draw.replay")
        );
    }

    #[test]
    fn empty_template_returns_none() {
        assert_eq!(
            render_upload_name("   ", &match_entry(), "Epic|me|0", "Primary"),
            None
        );
    }

    #[test]
    fn unknown_placeholders_are_left_untouched() {
        let rendered = substitute(
            "{MAP} {SCORE} {NOPE}",
            &fields(&match_entry(), "Epic|me|0", "x"),
        );
        assert_eq!(rendered, "DFH Stadium 3-1 {NOPE}");
    }

    #[test]
    fn playlist_names_cover_common_modes_and_fall_back() {
        assert_eq!(playlist_name(11), "Ranked Doubles");
        assert_eq!(playlist_name(30), "Snow Day");
        assert_eq!(playlist_name(999), "Playlist 999");
    }

    #[test]
    fn timezone_offset_is_respected() {
        let east = FixedOffset::east_opt(2 * 3600).unwrap();
        let rendered = substitute(
            "{HOUR}.{MIN}",
            &Fields::from_match(&match_entry(), "Epic|me|0", "x", &east).unwrap(),
        );
        assert_eq!(rendered, "16.30");
    }
}
