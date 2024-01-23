use views::media::MediaType;

pub struct Query {
    pub title: String,
    pub media_type: Option<MediaType>
}