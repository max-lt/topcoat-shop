//! Crawler fare: the journal's RSS feed, the sitemap and robots.txt.

use topcoat::context::Cx;
use topcoat::router::content::sitemap::{ChangeFrequency, Sitemap, SitemapUrl};
use topcoat::router::response::{IntoResponse, Response};
use topcoat::router::{route, Body};
use topcoat::Result;

use crate::app::context::{pool, public_origin};
use crate::app::journal::POSTS;
use crate::db;

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

/// Absolute locations rather than root-relative ones: the same build serves
/// localhost, workers.dev and the shop's own domain, so the origin comes
/// from the request instead of a base registered once on the router.
#[route(GET "/sitemap.xml")]
async fn sitemap(cx: &Cx) -> Result<Sitemap> {
    let origin = public_origin(cx);
    let at = |path: &str| SitemapUrl::new(format!("{origin}{path}"));

    Ok(Sitemap::new()
        .url(at("/").change_frequency(ChangeFrequency::Daily).priority(1.0))
        .url(at("/boutique").change_frequency(ChangeFrequency::Daily).priority(0.9))
        .url(at("/journal").change_frequency(ChangeFrequency::Weekly).priority(0.6))
        .urls(["/maison", "/aide", "/contact"].map(|path| {
            at(path).change_frequency(ChangeFrequency::Monthly).priority(0.4)
        }))
        .urls(["/cgv", "/mentions-legales"].map(|path| {
            at(path).change_frequency(ChangeFrequency::Yearly).priority(0.1)
        }))
        // Stock and prices move under a product page that keeps its URL.
        .urls(db::catalog(pool(cx), "", 3).await?.iter().map(|p| {
            at(&format!("/produit/{}", p.sku))
                .change_frequency(ChangeFrequency::Weekly)
                .priority(0.8)
        }))
        .urls(POSTS.map(|(slug, ..)| {
            at(&format!("/journal/{slug}")).change_frequency(ChangeFrequency::Yearly).priority(0.5)
        })))
}

#[route(GET "/robots.txt")]
async fn robots(cx: &Cx) -> Result<Plain> {
    Ok(Plain(format!(
        "User-agent: *\nAllow: /\nDisallow: /compte\nDisallow: /panier\nDisallow: /commander\n\
         \nSitemap: {}/sitemap.xml\n",
        public_origin(cx)
    )))
}
