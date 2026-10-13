//! Qualification reuses the production decoder and its regression tests.
#[path = "../../../nebula_app/src/platform/background_media/gif_decoder.rs"]
mod production;
pub use production::{Cursor, Decoded};
