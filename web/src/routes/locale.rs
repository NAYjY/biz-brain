//! T11: locale switcher endpoint.
//! POST /locale — sets the `locale` cookie and redirects back to the page.
//! The form submits `locale=th|en` and `redirect_to=<path>`.

use axum::{
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use serde::Deserialize;
use time::Duration;

use crate::i18n::Locale;

#[derive(Deserialize)]
pub struct LocaleForm {
    pub locale: String,
    pub redirect_to: Option<String>,
}

pub async fn set_locale(jar: CookieJar, Form(form): Form<LocaleForm>) -> Response {
    // Validate — only accept supported locales; ignore unknown values silently.
    let valid = Locale::from_str(&form.locale).is_some();

    let redirect = form
        .redirect_to
        .filter(|p| p.starts_with('/') && !p.contains("//"))
        .unwrap_or_else(|| "/".to_string());

    if !valid {
        return Redirect::to(&redirect).into_response();
    }

    let cookie = Cookie::build(("locale", form.locale))
        .path("/")
        .http_only(false) // readable by JS is fine; it's not sensitive
        .same_site(SameSite::Lax)
        .max_age(Duration::days(365))
        .build();

    (jar.add(cookie), Redirect::to(&redirect)).into_response()
}
