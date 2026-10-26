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

use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::{BodyLimit, Router, RouterBuilder, RouterBuilderDiscoverExt, TrailingSlash};
use topcoat::runtime::RouterBuilderRuntimeExt;
use topcoat::session::{RouterBuilderSessionExt, SessionConfig};

/// The router every host serves. The host adds what only it has: the
/// asset bundle and the SQLite pool natively, nothing at the edge.
pub fn router() -> RouterBuilder {
    Router::builder()
        // The sitemap names one form of every url. The other 308s to it, so a
        // link with a stray slash keeps its method and its body and lands on
        // the page that is indexed.
        .trailing_slash(TrailingSlash::Redirect)
        .runtime()
        .discover()
        .cookies()
        .sessions(SessionConfig::default())
        .layer(BodyLimit::max(app::admin::PHOTO_LIMIT).at("/admin/photo"))
}

// --- the Worker entry point

#[cfg(feature = "edge")]
mod worker_adapter {
    use std::sync::OnceLock;

    use http::{Method, StatusCode};
    use topcoat::router::{Body, Router};
    use worker::{
        console_log, event, Context, Env, HttpRequest, Result, ScheduleContext, ScheduledEvent,
    };

    /// Built on the first request a wasm instance serves, and kept for the
    /// ones after it: discovery walks every route the binary holds, and
    /// nothing in the router depends on the request.
    fn router() -> &'static Router {
        static ROUTER: OnceLock<Router> = OnceLock::new();
        ROUTER.get_or_init(|| crate::router().build())
    }

    /// The whole shop as a pure function: an http::Request in, the router's
    /// handle(), an http::Response out. No server underneath. Both bodies
    /// stream: an upload reaches the router as it arrives, and a page
    /// leaves frame by frame rather than after its slowest region.
    #[event(fetch)]
    async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<http::Response<Body>> {
        console_error_panic_hook::set_once();
        crate::db::install(env.d1("DB")?);
        crate::images::install(&env);

        // The Worker hands over the absolute url. The router is given what
        // a server would give it: the path and the query.
        let (mut head, body) = req.into_parts();
        if let Some(target) = head.uri.path_and_query().cloned() {
            head.uri = target.into();
        }
        let bodiless = head.method == Method::HEAD;

        let mut response =
            router().handle(http::Request::from_parts(head, Body::new(body))).await;

        // The Workers runtime refuses a body on these statuses, and a HEAD
        // answer must not carry the page it describes.
        if bodiless
            || matches!(
                response.status(),
                StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
            )
        {
            *response.body_mut() = Body::empty();
        }
        Ok(response)
    }

    /// The parcels march on their own, on the cron declared in
    /// wrangler.toml. A scheduled event has no request to answer, so a
    /// failure only has the log to land in.
    #[event(scheduled)]
    async fn tick(event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
        console_error_panic_hook::set_once();
        let Ok(d1) = env.d1("DB") else {
            console_log!("cron {}: no DB binding", event.cron());
            return;
        };
        crate::db::install(d1);
        match crate::db::advance_pending(crate::db::Db).await {
            Ok(moved) => console_log!("cron {}: {moved} orders advanced", event.cron()),
            Err(e) => console_log!("cron {}: {e}", event.cron()),
        }
    }
}
