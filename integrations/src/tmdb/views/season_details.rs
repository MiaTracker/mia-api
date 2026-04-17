use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct SeasonDetails {
    pub id: i32,
    pub air_date: Option<String>,
    pub episodes: Vec<TmdbEpisode>,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub season_number: Option<i32>,
    pub vote_average: Option<f32>,
}

#[derive(Deserialize, Clone)]
pub struct TmdbEpisode {
    pub id: i32,
    pub air_date: Option<String>,
    pub episode_number: Option<i32>,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<i32>,
    pub season_number: Option<i32>,
    pub still_path: Option<String>,
    pub vote_average: Option<f32>,
}
