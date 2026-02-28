use chrono::{DateTime, Local};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Checking,
    UpToDate,
    UpdatesAvailable,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateSource {
    Official,
    Aur,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUpdate {
    pub name: String,
    pub current: String,
    pub latest: String,
    pub source: UpdateSource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateSnapshot {
    pub official: Vec<PackageUpdate>,
    pub aur: Vec<PackageUpdate>,
}

impl UpdateSnapshot {
    pub fn total_count(&self) -> usize {
        self.official.len() + self.aur.len()
    }
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub status: Status,
    pub official_count: usize,
    pub aur_count: usize,
    pub total_count: usize,
    pub last_checked: Option<DateTime<Local>>,
    pub last_error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            status: Status::Checking,
            official_count: 0,
            aur_count: 0,
            total_count: 0,
            last_checked: None,
            last_error: None,
        }
    }
}

impl AppState {
    pub fn from_snapshot(snapshot: &UpdateSnapshot, checked_at: DateTime<Local>) -> Self {
        let total = snapshot.total_count();
        let status = if total == 0 {
            Status::UpToDate
        } else {
            Status::UpdatesAvailable
        };

        Self {
            status,
            official_count: snapshot.official.len(),
            aur_count: snapshot.aur.len(),
            total_count: total,
            last_checked: Some(checked_at),
            last_error: None,
        }
    }

    pub fn with_error(mut self, message: String, checked_at: DateTime<Local>) -> Self {
        self.status = Status::Error;
        self.last_checked = Some(checked_at);
        self.last_error = Some(message);
        self
    }

    pub fn with_checking(mut self) -> Self {
        self.status = Status::Checking;
        self.last_error = None;
        self
    }
}
