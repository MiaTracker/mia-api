use infrastructure::{AssistantConfiguration, Configuration, DataPolicy, DbConfiguration, ImagesConfiguration, JwtConfiguration, LogLevel, LoggingConfiguration, MediaConfiguration, OpenRouterConfiguration, TMDBConfiguration};
use integrations::tmdb;

/// Application configuration with fixed values; nothing in it points at a real resource.
pub fn test_config() -> Configuration {
    Configuration {
        jwt: JwtConfiguration { secret: "test-secret".into() },
        db: DbConfiguration { connection_url: "postgres://unused".into(), schema: "public".into() },
        tmdb: TMDBConfiguration { authorization_token: "test-token".into(), refresh_interval: None },
        logging: LoggingConfiguration { level: LogLevel::Debug },
        media: MediaConfiguration { unset_title: "Unknown".into() },
        images: ImagesConfiguration { store_path: "/nonexistent-test-store".into() },
        openrouter: OpenRouterConfiguration { api_key: "test-key".into(), model: "test/model".into(), max_tokens: 16384, reasoning: None, data_policy: DataPolicy::Allow },
        assistant: AssistantConfiguration { mia_search_limit: 30, mia_details_limit: 3, max_tool_rounds: 2 },
    }
}

/// TMDB configuration as TMDB's `/configuration` endpoint would return it, with fixed values.
pub fn test_tmdb_configuration() -> tmdb::views::Configuration {
    fn sizes(slugs: &[&str]) -> Vec<String> {
        slugs.iter().map(|s| s.to_string()).collect()
    }

    tmdb::views::Configuration {
        images: tmdb::views::ImagesConfiguration {
            base_url: "http://image.tmdb.test/t/p/".into(),
            secure_base_url: "https://image.tmdb.test/t/p/".into(),
            backdrop_sizes: sizes(&["w300", "w780", "w1280", "original"]),
            logo_sizes: sizes(&["w45", "w92", "w154", "w185", "w300", "w500", "original"]),
            poster_sizes: sizes(&["w92", "w154", "w185", "w342", "w500", "w780", "original"]),
            profile_sizes: sizes(&["w45", "w185", "h632", "original"]),
            still_sizes: sizes(&["w92", "w185", "w300", "original"]),
        },
        change_keys: Vec::new(),
    }
}
