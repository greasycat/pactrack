use std::fs;
use std::io;
use std::path::PathBuf;

use crate::state::Status;

const CHECKING_XPM: &str = r#"/* XPM */
static char * checking_xpm[] = {
"16 16 2 1",
"  c None",
". c #f4b400",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................"
};
"#;

const UP_TO_DATE_XPM: &str = r#"/* XPM */
static char * uptodate_xpm[] = {
"16 16 2 1",
"  c None",
". c #34a853",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................"
};
"#;

const UPDATES_XPM: &str = r#"/* XPM */
static char * updates_xpm[] = {
"16 16 2 1",
"  c None",
". c #1a73e8",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................"
};
"#;

const ERROR_XPM: &str = r#"/* XPM */
static char * error_xpm[] = {
"16 16 2 1",
"  c None",
". c #d93025",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................",
"................"
};
"#;

pub fn install_fallback_icons() -> io::Result<PathBuf> {
    let base = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("pactrack").join("icons");
    fs::create_dir_all(&dir)?;

    fs::write(dir.join("pactrack-checking.xpm"), CHECKING_XPM)?;
    fs::write(dir.join("pactrack-up-to-date.xpm"), UP_TO_DATE_XPM)?;
    fs::write(dir.join("pactrack-updates-available.xpm"), UPDATES_XPM)?;
    fs::write(dir.join("pactrack-error.xpm"), ERROR_XPM)?;

    Ok(dir)
}

pub fn icon_candidates(status: &Status) -> (&'static str, &'static str) {
    match status {
        Status::Checking => ("view-refresh-symbolic", "pactrack-checking"),
        Status::UpToDate => ("emblem-default", "pactrack-up-to-date"),
        Status::UpdatesAvailable => ("software-update-available", "pactrack-updates-available"),
        Status::Error => ("dialog-error", "pactrack-error"),
    }
}
