//! Crawler fare: the journal's RSS feed, the sitemap and robots.txt.
//! Hand-rolled XML -- three tags do not justify a dependency.

use topcoat::context::Cx;
use topcoat::router::{route, Body, IntoResponse, Response};
use topcoat::Result;

use crate::app::context::{pool, public_origin};
use crate::app::journal::POSTS;
use crate::db;

struct Xml(String);

impl IntoResponse for Xml {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(Body::from(self.0))?)
    }
}

struct Rss(String);

impl IntoResponse for Rss {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header("Content-Type", "application/rss+xml; charset=utf-8")
            .body(Body::from(self.0))?)
    }
}

struct Plain(String);

impl IntoResponse for Plain {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        Ok(Response::builder()
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(Body::from(self.0))?)
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[route(GET "/journal/flux.xml")]
async fn feed(cx: &Cx) -> Result<Rss> {
    let origin = public_origin(cx);
    let mut items = String::new();
    for (slug, _date, tag, title, lede) in POSTS {
        items.push_str(&format!(
            "<item><title>{}</title><link>{origin}/journal/{slug}</link>\
             <guid>{origin}/journal/{slug}</guid><category>{}</category>\
             <description>{}</description></item>",
            escape(title),
            escape(tag),
            escape(lede),
        ));
    }
    Ok(Rss(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><rss version=\"2.0\"><channel>\
         <title>Le journal de Bernard</title><link>{origin}/journal</link>\
         <description>Les matières, l'atelier, les décisions qu'on assume.</description>\
         {items}</channel></rss>"
    )))
}

#[route(GET "/sitemap.xml")]
async fn sitemap(cx: &Cx) -> Result<Xml> {
    let origin = public_origin(cx);
    let mut urls = String::new();
    for path in
        ["/", "/boutique", "/journal", "/maison", "/aide", "/contact", "/cgv", "/mentions-legales"]
    {
        urls.push_str(&format!("<url><loc>{origin}{path}</loc></url>"));
    }
    for p in db::catalog(pool(cx), "", 3).await? {
        urls.push_str(&format!("<url><loc>{origin}/produit/{}</loc></url>", p.sku));
    }
    for (slug, ..) in POSTS {
        urls.push_str(&format!("<url><loc>{origin}/journal/{slug}</loc></url>"));
    }
    Ok(Xml(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{urls}</urlset>"
    )))
}

#[route(GET "/robots.txt")]
async fn robots(cx: &Cx) -> Result<Plain> {
    Ok(Plain(format!(
        "User-agent: *\nAllow: /\nDisallow: /compte\nDisallow: /panier\nDisallow: /commander\n\
         \nSitemap: {}/sitemap.xml\n",
        public_origin(cx)
    )))
}
