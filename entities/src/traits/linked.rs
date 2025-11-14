use sea_orm::{LinkDef, Linked, RelationTrait};
use crate::media;
use crate::prelude::{Images, Media};

pub struct MediaBackdrops;

impl Linked for MediaBackdrops {
    type FromEntity = Media;
    type ToEntity = Images;

    fn link(&self) -> Vec<LinkDef> {
        vec![media::Relation::Images2.def()]
    }
}

pub struct MediaPosters;

impl Linked for MediaPosters {
    type FromEntity = Media;
    type ToEntity = Images;

    fn link(&self) -> Vec<LinkDef> {
        vec![media::Relation::Images1.def()]
    }
}