pub trait IntoActiveModel<T> {
    fn into_active_model(self) -> T;
}

pub trait IntoView<T> {
    fn into_view(self) -> T;
}