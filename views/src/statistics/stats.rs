use sea_orm::FromQueryResult;
use serde::Serialize;
use utoipa::ToSchema;
use crate::media::MediaIndex;

#[derive(Serialize, ToSchema)]
pub struct Stats {
    pub media: MediaStats,
    pub logs: LogStats,
    pub genres: Vec<ComparativeStats>,
    pub languages: Vec<ComparativeStats>,
    pub most_watched: CategoryStats,
    pub highest_rated: CategoryStats,
    pub average_rating: AvgRatingStats
}

#[derive(Serialize, ToSchema)]
pub struct MediaStats {
    pub count: u64,
    pub movies: u64,
    pub series: u64
}

#[derive(Serialize, ToSchema)]
pub struct LogStats {
    pub logs: u64,
    pub completed: u64,
    pub uncompleted: u64
}

#[derive(Serialize, ToSchema)]
pub struct CategoryStats {
    pub movie: Option<MediaIndex>,
    pub series: Option<MediaIndex>
}

#[derive(Serialize, FromQueryResult, ToSchema)]
pub struct ComparativeStats {
    pub name: String,
    pub count: i64
}

#[derive(Serialize, ToSchema)]
pub struct AvgRatingStats {
    pub overall: Option<f64>,
    pub movies: Option<f64>,
    pub series: Option<f64>
}