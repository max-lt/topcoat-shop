//! Bernard's shop: catalog, cart, accounts, order tracking and a back
//! office, served by Topcoat. The same pages run on two hosts: a native
//! binary over tokio and SQLite (see main.rs), and a Cloudflare Worker
//! over D1 (the fetch adapter below). Host-specific code is confined to
//! db's backends, the images module and this file.

pub mod app;
pub mod db;

pub mod bundle;

#[cfg(feature = "native")]
pub mod design;

#[cfg(feature = "native")]
#[path = "images/native.rs"]
pub mod images;

#[cfg(feature = "edge")]
#[path = "images/edge.rs"]
pub mod images;

// --- the Worker entry point

#[cfg(feature = "edge")]
mod worker_adapter {
    use http_body_util::BodyExt;
    use topcoat::cookie::RouterBuilderCookieExt;
    use topcoat::router::{BodyLimit, Body, Router, RouterBuilderDiscoverExt};
    use topcoat::session::{RouterBuilderSessionExt, SessionConfig};
    use worker::{event, Context, Env, Error, Headers, Request, Response, Result};

    /// The whole shop as a pure function: worker::Request in, the router's
    /// handle(), worker::Response out. No server underneath.
    #[event(fetch)]
    async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
        console_error_panic_hook::set_once();
        crate::db::install(env.d1("DB")?);
        crate::images::install(&env);

        let url = req.url()?;
        let target = format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let mut request = http::Request::builder().method(req.method().as_ref()).uri(target);
        for (name, value) in req.headers().entries() {
            request = request.header(&name, &value);
        }
        let body = req.clone()?.bytes().await?;
        let request =
            request.body(Body::from(body)).map_err(|e| Error::RustError(e.to_string()))?;

        let router = Router::builder()
            .discover()
            .cookies()
            .sessions(SessionConfig::default())
            .layer(BodyLimit::max(crate::app::admin::PHOTO_LIMIT).at("/admin/photo"))
            .build();
        let response = router.handle(request).await;

        let (head, body) = response.into_parts();
        let bytes = body
            .collect()
            .await
            .map_err(|e| Error::RustError(e.to_string()))?
            .to_bytes();
        let headers = Headers::new();
        for (name, value) in head.headers.iter() {
            if let Ok(v) = value.to_str() {
                headers.append(name.as_str(), v)?;
            }
        }
        Ok(Response::from_bytes(bytes.to_vec())?
            .with_status(head.status.as_u16())
            .with_headers(headers))
    }
}
