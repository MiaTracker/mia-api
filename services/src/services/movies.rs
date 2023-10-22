use std::env;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryFilter};
use entities::{titles, user_media};
use entities::prelude::{Media, Movies, Titles, UserMedia};
use entities::sea_orm_active_enums::MediaType;
use views::media::MediaIndex;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let user_media = UserMedia::find().filter(user_media::Column::UserId.eq(user.id)).all(db).await?;
    let mut indexes = Vec::with_capacity(user_media.len());
    for um in user_media {
        let media = um.find_related(Media).one(db).await?;
        if media.is_none() {
            return Err(SrvErr::Internal("User media exists without a media reference!".to_string()));
        }
        let media = media.unwrap();

        let title = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
        let title = if let Some(title) = title {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let stars;
        if media.r#type == MediaType::Movie {
            let movie = media.find_related(Movies).one(db).await?;
            if let Some(movie) = movie {
                stars = movie.stars;
            } else {
                return Err(SrvErr::Internal("Media of type movie exists without a movie reference!".to_string()));
            }
        } else {
            stars = None;
            //TODO: series
        }


        let index = MediaIndex {
            id: media.id,
            r#type: views::media::MediaType::from(media.r#type),
            poster_path: media.poster_path,
            stars,
            title,
        };
        indexes.push(index);
    }

    Ok(indexes)
}