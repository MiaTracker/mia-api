use serde::Serialize;
use crate::media::MediaIndex;

#[derive(Serialize)]
pub struct Stats {
    pub media: MediaStats,
    pub logs: LogStats,
    pub most_watched: MostWatchedStats
}

#[derive(Serialize)]
pub struct MediaStats {
    pub count: u64,
    pub movies: u64,
    pub series: u64
}

#[derive(Serialize)]
pub struct LogStats {
    pub logs: u64,
    pub completed: u64,
    pub uncompleted: u64
}

#[derive(Serialize)]
pub struct MostWatchedStats {
    pub movie: Option<MediaIndex>,
    pub series: Option<MediaIndex>
}