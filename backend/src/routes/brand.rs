//! Public brand assets. Served from the outer router (outside the
//! auth-protected /api/v1 subtree) so the web app and external surfaces can
//! hotlink the icon without credentials.

use axum::{http::header, response::IntoResponse};

/// The app icon mark — kept byte-identical to the web app's favicon
/// (black field, red mark). Embedded as a static string so the response
/// is allocation-free and cacheable.
pub const ICON_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect width='64' height='64' fill='#050505'/><path fill='#E53935' d='M32 15 L19 46 L15 46 L15 49 L26 49 Z M32 15 L45 46 L49 46 L49 49 L38 49 Z M32 15 L33.5 20 L33.5 49 L30.5 49 L30.5 20 Z'/></svg>";

pub async fn icon_svg() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], ICON_SVG)
}
