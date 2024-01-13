pub fn set_panic_hook() {
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();
}