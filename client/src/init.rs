use wasm_bindgen::prelude::wasm_bindgen;
use crate::infrastructure;

#[wasm_bindgen]
pub fn init() {
    infrastructure::set_panic_hook();
}