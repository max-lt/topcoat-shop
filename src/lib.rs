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
        console_log, event, AbortSignal, Context, Env, HttpRequest, Result, ScheduleContext,
        ScheduledEvent,
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
        // The bindings ride with the request: pool(cx) and the photo
        // routes read them from its context.
        let bindings = (crate::db::Db::new(env.d1("DB")?), crate::images::Bindings::new(&env));

        // The Worker hands over the absolute url. The router is given what
        // a server would give it: the path and the query.
        let (mut head, body) = req.into_parts();
        if let Some(target) = head.uri.path_and_query().cloned() {
            head.uri = target.into();
        }
        let bodiless = head.method == Method::HEAD;
        let signal = head.extensions.get::<AbortSignal>().cloned();

        let mut response =
            router().handle_with(http::Request::from_parts(head, Body::new(body)), bindings).await;

        // The Workers runtime refuses a body on these statuses, and a HEAD
        // answer must not carry the page it describes.
        if bodiless
            || matches!(
                response.status(),
                StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
            )
        {
            *response.body_mut() = Body::empty();
        } else if let Some(signal) = signal {
            let body = std::mem::take(response.body_mut());
            *response.body_mut() = disconnect::until(signal, body)?;
        }
        Ok(response)
    }

    /// A reader who leaves mid-page should not cost the rest of the page.
    /// The runtime does not always cancel the response stream when the
    /// client goes away, but it always fires the request's abort signal.
    mod disconnect {
        use std::cell::RefCell;
        use std::pin::Pin;
        use std::rc::Rc;
        use std::task::{Context, Poll, Waker};

        use http_body::{Body as _, Frame, SizeHint};
        use topcoat::router::Body;
        use worker::send::SendWrapper;
        use worker::wasm_bindgen::closure::Closure;
        use worker::wasm_bindgen::JsCast;
        use worker::AbortSignal;

        /// What the stream reads from, until the abort takes it away.
        struct Pending {
            body: Option<Body>,
            waker: Option<Waker>,
        }

        /// The body, dropped on abort: dropping it drops the render
        /// futures behind it, and the stream then ends.
        pub fn until(signal: AbortSignal, body: Body) -> worker::Result<Body> {
            if signal.aborted() {
                return Ok(Body::empty());
            }
            if body.is_end_stream() {
                return Ok(body);
            }

            let pending = Rc::new(RefCell::new(Pending { body: Some(body), waker: None }));
            let weak = Rc::downgrade(&pending);
            let on_abort = Closure::<dyn FnMut()>::new(move || {
                let Some(pending) = weak.upgrade() else { return };
                let (body, waker) = {
                    let mut pending = pending.borrow_mut();
                    (pending.body.take(), pending.waker.take())
                };
                // Dropped outside the borrow: a render future's destructor
                // may call back into the host.
                drop(body);
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
            signal.add_event_listener_with_callback("abort", on_abort.as_ref().unchecked_ref())?;

            Ok(Body::new(Guarded {
                pending: SendWrapper::new(pending),
                _listener: SendWrapper::new(Listener { signal, on_abort }),
            }))
        }

        /// Takes the listener off the signal once the response is done.
        struct Listener {
            signal: AbortSignal,
            on_abort: Closure<dyn FnMut()>,
        }

        impl Drop for Listener {
            fn drop(&mut self) {
                let _ = self.signal.remove_event_listener_with_callback(
                    "abort",
                    self.on_abort.as_ref().unchecked_ref(),
                );
            }
        }

        // A Worker runs on one thread; SendWrapper says so to Body::new.
        struct Guarded {
            pending: SendWrapper<Rc<RefCell<Pending>>>,
            _listener: SendWrapper<Listener>,
        }

        impl http_body::Body for Guarded {
            type Data = <Body as http_body::Body>::Data;
            type Error = <Body as http_body::Body>::Error;

            fn poll_frame(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
                let mut pending = self.pending.borrow_mut();
                pending.waker = Some(cx.waker().clone());
                let Some(body) = pending.body.as_mut() else {
                    return Poll::Ready(None);
                };
                let frame = Pin::new(body).poll_frame(cx);
                if let Poll::Ready(None) = frame {
                    pending.body = None;
                    pending.waker = None;
                }
                frame
            }

            fn is_end_stream(&self) -> bool {
                self.pending.borrow().body.as_ref().is_none_or(|body| body.is_end_stream())
            }

            fn size_hint(&self) -> SizeHint {
                match &self.pending.borrow().body {
                    Some(body) => body.size_hint(),
                    None => SizeHint::with_exact(0),
                }
            }
        }
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
        match crate::db::advance_pending(&crate::db::Db::new(d1)).await {
            Ok(moved) => console_log!("cron {}: {moved} orders advanced", event.cron()),
            Err(e) => console_log!("cron {}: {e}", event.cron()),
        }
    }
}
