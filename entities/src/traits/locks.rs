use sea_orm::Set;
use crate::{media_locks, movie_locks, series_locks};

pub trait ToLocks {
    fn to_locks(&self) -> Vec<&'static str>;
}

pub trait SetLock {
    fn has_lock(property: &str) -> bool;
    fn set_lock(&mut self, property: &str, locked: bool);
}

impl ToLocks for Option<media_locks::Model> {
    fn to_locks(&self) -> Vec<&'static str> {
        let mut list = Vec::new();
        if let Some(locks) = self {
            if locks.backdrop_path { list.push(stringify!(backdrop_path)) }
            if locks.homepage { list.push(stringify!(homepage)) }
            if locks.imdb_id { list.push(stringify!(imdb_id)) }
            if locks.overview { list.push(stringify!(overview)) }
            if locks.poster_path { list.push(stringify!(poster_path)) }
            if locks.tmdb_vote_average { list.push(stringify!(tmdb_vote_average)) }
            if locks.original_language { list.push(stringify!(original_language)) }
        }
        list
    }
}

impl SetLock for media_locks::ActiveModel {
    fn has_lock(property: &str) -> bool {
        property == stringify!(backdrop_path) ||
        property == stringify!(homepage) ||
        property == stringify!(imdb_id) ||
        property == stringify!(overview) ||
        property == stringify!(poster_path) ||
        property == stringify!(tmdb_vote_average) ||
        property == stringify!(original_language)
    }

    fn set_lock(&mut self, property: &str, locked: bool) {
        if property == stringify!(backdrop_path) { self.backdrop_path = Set(locked) }
        else if property == stringify!(homepage) { self.homepage = Set(locked) }
        else if property == stringify!(imdb_id) { self.imdb_id = Set(locked) }
        else if property == stringify!(overview) { self.overview = Set(locked) }
        else if property == stringify!(poster_path) { self.poster_path = Set(locked) }
        else if property == stringify!(tmdb_vote_average) { self.tmdb_vote_average = Set(locked) }
        else if property == stringify!(original_language) { self.original_language = Set(locked) }
    }
}

impl ToLocks for Option<movie_locks::Model> {
    fn to_locks(&self) -> Vec<&'static str> {
        let mut list = Vec::new();
        if let Some(locks) = self {
            if locks.release_date { list.push(stringify!(release_date)) }
            if locks.runtime { list.push(stringify!(runtime)) }
            if locks.status { list.push(stringify!(status)) }
        }
        list
    }
}

impl SetLock for movie_locks::ActiveModel {
    fn has_lock(property: &str) -> bool {
        property == stringify!(release_date) ||
        property == stringify!(runtime) ||
        property == stringify!(status)
    }

    fn set_lock(&mut self, property: &str, locked: bool) {
        if property == stringify!(release_date) { self.release_date = Set(locked) }
        else if property == stringify!(runtime) { self.runtime = Set(locked) }
        else if property == stringify!(status) { self.status = Set(locked) }
    }
}

impl ToLocks for Option<series_locks::Model> {
    fn to_locks(&self) -> Vec<&'static str> {
        let mut list = Vec::new();
        if let Some(locks) = self {
            if locks.first_air_date { list.push(stringify!(first_air_date)) }
            if locks.number_of_episodes { list.push(stringify!(number_of_episodes)) }
            if locks.number_of_seasons { list.push(stringify!(number_of_seasons)) }
            if locks.status { list.push(stringify!(status)) }
            if locks.r#type { list.push(stringify!(type)) }
        }
        list
    }
}

impl SetLock for series_locks::ActiveModel {
    fn has_lock(property: &str) -> bool {
        property == stringify!(first_air_date) ||
            property == stringify!(number_of_episodes) ||
            property == stringify!(number_of_seasons) ||
            property == stringify!(status) ||
            property == stringify!(type)
    }

    fn set_lock(&mut self, property: &str, locked: bool) {
        if property == stringify!(first_air_date) { self.first_air_date = Set(locked) }
        else if property == stringify!(number_of_episodes) { self.number_of_episodes = Set(locked) }
        else if property == stringify!(number_of_seasons) { self.number_of_seasons = Set(locked) }
        else if property == stringify!(status) { self.status = Set(locked) }
        else if property == stringify!(type) { self.r#type = Set(locked) }
    }
}