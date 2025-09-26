use entities::languages;
use views::images::Image;

pub trait IntoActiveModel<T> {
    fn into_active_model(self) -> T;
}

pub trait IntoView<T> {
    fn into_view(self) -> T;
}

pub trait SortCompare {
    fn sort_compare(&self, other: &Self) -> std::cmp::Ordering;
}

pub trait IntoImage {
    fn into_image(self, default_path: &Option<String>, languages: &Vec<languages::Model>) -> Image;
}