//! The shop answered through its own router, in process. `Router::handle`
//! is a pure function, so none of this needs a server or a port.
//!
//! Needs the asset bundle the binary needs: `topcoat asset bundle --bin
//! topcoat-shop`. The head of every page asks for the stylesheet and the
//! runtime script, and both come from there.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use topcoat::asset::{AssetBundle, RouterBuilderAssetExt};
use topcoat::cookie::RouterBuilderCookieExt;
use topcoat::router::request::Request;
use topcoat::router::response::Response;
use topcoat::router::{Body, Router, RouterBuilderDiscoverExt, StatusCode, to_bytes};
use topcoat::runtime::RouterBuilderRuntimeExt;
use topcoat::session::{RouterBuilderSessionExt, SessionConfig};

use topcoat_shop::db;

const LIMIT: usize = 32 * 1024 * 1024;

// --- harness

/// A router over a database of its own, so tests can run in parallel.
struct Shop {
    router: Router,
    database: PathBuf,
    /// Whatever the shop has set on this visitor so far, as a Cookie header.
    jar: Vec<(String, String)>,
}

impl Shop {
    async fn open() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let database =
            std::env::temp_dir().join(format!("topcoat-shop-test-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&database);

        let pool = db::connect(&database.to_string_lossy())
            .await
            .expect("database");
        let router = Router::builder()
            .runtime()
            .discover()
            .assets(asset_bundle())
            .cookies()
            .sessions(SessionConfig::default())
            .app_context(pool)
            .build();

        Self {
            router,
            database,
            jar: Vec::new(),
        }
    }

    async fn get(&mut self, url: &str) -> Page {
        self.send("GET", url, None).await
    }

    /// A form submission, urlencoded the way a browser sends one.
    async fn post(&mut self, url: &str, fields: &[(&str, &str)]) -> Page {
        let body = fields
            .iter()
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.send("POST", url, Some(body)).await
    }

    async fn send(&mut self, method: &str, url: &str, form: Option<String>) -> Page {
        let mut request = Request::builder().method(method).uri(url);
        if form.is_some() {
            request = request.header("content-type", "application/x-www-form-urlencoded");
        }
        if !self.jar.is_empty() {
            let header = self
                .jar
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            request = request.header("cookie", header);
        }
        let body = form.map_or_else(Body::empty, Body::from);
        let response = self
            .router
            .handle(request.body(body).expect("request"))
            .await;
        self.keep_cookies(&response);

        let status = response.status();
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = to_bytes(response.into_body(), LIMIT).await.expect("body");
        Page {
            status,
            location,
            html: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    /// The jar a browser would keep: a later Set-Cookie replaces an earlier
    /// one, and an expiry in the past drops it.
    fn keep_cookies(&mut self, response: &Response) {
        for value in response.headers().get_all("set-cookie") {
            let Ok(text) = value.to_str() else { continue };
            let pair = text.split(';').next().unwrap_or_default();
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            self.jar.retain(|(k, _)| k != name.trim());
            let cleared = text.contains("Max-Age=0") || text.contains("1970");
            if !cleared {
                self.jar
                    .push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    }

    /// Signs up, which is also the only way to get a session.
    async fn sign_up(&mut self, email: &str) -> Page {
        self.post(
            "/inscription",
            &[
                ("email", email),
                ("name", "Testeur"),
                ("password", "motdepasse123"),
            ],
        )
        .await
    }

    fn pool(&self) -> String {
        self.database.to_string_lossy().into_owned()
    }
}

impl Drop for Shop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.database);
    }
}

struct Page {
    status: StatusCode,
    location: Option<String>,
    html: String,
}

impl Page {
    fn assert_ok(&self, url: &str) -> &Self {
        assert_eq!(
            self.status,
            StatusCode::OK,
            "{url} answered {}",
            self.status
        );
        self
    }

    fn contains(&self, needle: &str) -> bool {
        self.html.contains(needle)
    }

    /// How many times `needle` appears. The shop minifies to one line, so
    /// counting lines would count one.
    fn count(&self, needle: &str) -> usize {
        self.html.matches(needle).count()
    }
}

/// The bundle sits next to the binary. A test binary lives one directory
/// deeper, under `deps/`, so its own parent is not where to look.
fn asset_bundle() -> AssetBundle {
    let exe = std::env::current_exe().expect("test binary path");
    let dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("target directory")
        .join("assets");
    AssetBundle::load_dir(&dir).unwrap_or_else(|e| {
        panic!("{e}: run `topcoat asset bundle --bin topcoat-shop` before the tests")
    })
}

fn urlencode(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Every SKU the seeded catalog puts on the shop page.
fn skus(page: &Page) -> Vec<String> {
    let mut found = Vec::new();
    for (index, _) in page.html.match_indices("href=\"/produit/") {
        let rest = &page.html[index + "href=\"/produit/".len()..];
        let sku = &rest[..rest.find('"').unwrap_or(0)];
        if !sku.is_empty() && !found.iter().any(|seen| seen == sku) {
            found.push(sku.to_string());
        }
    }
    found
}

// --- the shop a visitor sees

#[tokio::test]
async fn every_public_page_answers() {
    let mut shop = Shop::open().await;
    for url in [
        "/",
        "/boutique",
        "/maison",
        "/aide",
        "/contact",
        "/cgv",
        "/mentions-legales",
        "/journal",
        "/recherche",
        "/recherche?q=mug",
        "/panier",
        "/connexion",
        "/sitemap.xml",
        "/journal/flux.xml",
        "/robots.txt",
    ] {
        shop.get(url).await.assert_ok(url);
    }
}

#[tokio::test]
async fn every_product_in_the_catalog_answers() {
    let mut shop = Shop::open().await;
    let shelf = shop.get("/boutique").await;
    shelf.assert_ok("/boutique");
    let catalog = skus(&shelf);
    assert!(
        catalog.len() > 10,
        "the catalog seeded {} products",
        catalog.len()
    );

    for sku in catalog {
        let url = format!("/produit/{sku}");
        shop.get(&url).await.assert_ok(&url);
    }
}

/// The page reads its own signal on the server, so a query string still
/// renders the results without the browser running anything.
#[tokio::test]
async fn the_search_page_renders_its_results_from_the_query_string() {
    let mut shop = Shop::open().await;

    let found = shop.get("/recherche?q=mug").await;
    found.assert_ok("/recherche?q=mug");
    assert!(
        found.contains("/produit/COQ-MUG"),
        "the mug is missing from its own search"
    );
    assert!(found.contains("résultat"), "the count is missing");

    // An empty field is a selection, not an empty page.
    let bare = shop.get("/recherche").await;
    bare.assert_ok("/recherche");
    assert!(bare.contains("Une sélection pour commencer"));

    let nothing = shop.get("/recherche?q=zzzzzz").await;
    nothing.assert_ok("/recherche?q=zzzzzz");
    assert!(
        nothing.contains("Rien pour"),
        "a search with no hit says nothing found"
    );
}

#[tokio::test]
async fn a_missing_page_is_a_404_wearing_the_shell() {
    let mut shop = Shop::open().await;
    let page = shop.get("/rien-du-tout").await;

    assert_eq!(page.status, StatusCode::NOT_FOUND);
    assert!(page.contains("Coquille vide"), "the 404 body is missing");
    // The shell, not a bare error page: the header and the footer are both
    // outside the slot the error boundary wraps.
    assert!(page.contains("Aller au contenu"), "the 404 lost the shell");
    assert!(
        page.contains("Livraison offerte"),
        "the 404 lost the banner"
    );
}

#[tokio::test]
async fn a_trailing_slash_redirects_to_the_declared_path() {
    let mut shop = Shop::open().await;
    let page = shop.get("/boutique/").await;

    assert_eq!(page.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(page.location.as_deref(), Some("/boutique"));
}

/// The hero does not wait on the queries below it. The three shelves are
/// live regions, so their markup reaches the stream after the part the
/// visitor came for.
#[tokio::test]
async fn the_product_page_sends_its_hero_before_the_shelves() {
    let mut shop = Shop::open().await;
    let page = shop.get("/produit/COQ-TSHIRT").await;
    page.assert_ok("/produit/COQ-TSHIRT");

    let hero = page
        .html
        .find("Ajouter au panier")
        .expect("the hero is missing");
    let reviews = page
        .html
        .find("id=\"avis\"")
        .expect("the reviews section is missing");
    assert!(hero < reviews, "the reviews came before the hero");

    // A suspended region puts its fallback in the stream and its content
    // after, so each heading goes out twice. Rendered inline it would go out
    // once, which is what tells the two apart.
    assert_eq!(page.count("Les avis"), 2, "the reviews did not stream");
    assert_eq!(
        page.count("À voir aussi"),
        2,
        "the related shelf did not stream"
    );
    assert!(
        page.contains("animate-pulse"),
        "no shelf placeholder went out"
    );
}

/// The shelf is fed by a cookie the page writes, and a single query. It
/// shows what came before this page, never this page.
#[tokio::test]
async fn the_seen_shelf_follows_the_visitor() {
    let mut shop = Shop::open().await;

    let first = shop.get("/produit/COQ-MUG").await;
    first.assert_ok("/produit/COQ-MUG");
    assert!(
        !first.contains("Déjà regardés"),
        "the first page shows a shelf of nothing"
    );

    let second = shop.get("/produit/COQ-TSHIRT").await;
    second.assert_ok("/produit/COQ-TSHIRT");
    assert_eq!(second.count("Déjà regardés"), 2, "the shelf did not stream");
    assert!(
        second.contains("/produit/COQ-MUG"),
        "the mug is missing from the shelf"
    );
}

/// The nav marks the page being served, and marks it from the route the
/// router matched rather than from the request path.
#[tokio::test]
async fn the_nav_marks_the_page_being_served() {
    let mut shop = Shop::open().await;

    // Two links per page, the desktop nav and the mobile one.
    let shelf = shop.get("/boutique").await;
    shelf.assert_ok("/boutique");
    assert_eq!(
        shelf.count("aria-current=\"page\""),
        2,
        "the shop is not marked"
    );

    let journal = shop.get("/journal").await;
    assert_eq!(
        journal.count("aria-current=\"page\""),
        2,
        "the journal is not marked"
    );

    // A filtered shop is still the shop: the query does not move the mark.
    let filtered = shop.get("/boutique?categorie=Vestiaire").await;
    filtered.assert_ok("/boutique?categorie=Vestiaire");
    assert_eq!(
        filtered.count("aria-current=\"page\""),
        2,
        "a filter lost the mark"
    );

    // A page outside the nav marks nothing, and neither does a 404.
    assert_eq!(shop.get("/panier").await.count("aria-current=\"page\""), 0);
    assert_eq!(
        shop.get("/rien-du-tout")
            .await
            .count("aria-current=\"page\""),
        0
    );
}

// --- accounts

#[tokio::test]
async fn a_visitor_can_sign_up_sign_out_and_sign_back_in() {
    let mut shop = Shop::open().await;

    let page = shop.sign_up("nouvelle@bernard.sh").await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert_eq!(page.location.as_deref(), Some("/compte"));
    shop.get("/compte").await.assert_ok("/compte");

    let page = shop.post("/deconnexion", &[]).await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert_eq!(
        shop.get("/compte").await.status,
        StatusCode::TEMPORARY_REDIRECT
    );

    let page = shop
        .post(
            "/connexion/verifier",
            &[
                ("email", "nouvelle@bernard.sh"),
                ("password", "motdepasse123"),
            ],
        )
        .await;
    assert_eq!(page.location.as_deref(), Some("/compte"));
    shop.get("/compte").await.assert_ok("/compte");
}

#[tokio::test]
async fn the_wrong_password_does_not_open_a_session() {
    let mut shop = Shop::open().await;
    shop.sign_up("titulaire@bernard.sh").await;
    shop.post("/deconnexion", &[]).await;

    let page = shop
        .post(
            "/connexion/verifier",
            &[("email", "titulaire@bernard.sh"), ("password", "faux")],
        )
        .await;

    // The POST is rewritten back onto its own page as a GET, so the answer is
    // the form again with the reason on it. Nothing redirects, and the reason
    // is not in the URL where a bookmark would keep it.
    page.assert_ok("the refused sign-in");
    assert_eq!(page.location, None, "a refused sign-in redirected");
    assert!(
        page.contains("Identifiants incorrects."),
        "the page does not say why"
    );
    assert!(
        page.contains("J'ai déjà un compte"),
        "the form did not come back"
    );
    assert_eq!(
        shop.get("/compte").await.status,
        StatusCode::TEMPORARY_REDIRECT
    );
}

#[tokio::test]
async fn an_email_is_taken_only_once() {
    let mut shop = Shop::open().await;
    shop.sign_up("unique@bernard.sh").await;
    shop.post("/deconnexion", &[]).await;

    let page = shop.sign_up("unique@bernard.sh").await;
    page.assert_ok("the refused sign-up");
    assert!(
        page.contains("Cet email a déjà un compte."),
        "the page does not say why"
    );
    assert_eq!(
        shop.get("/compte").await.status,
        StatusCode::TEMPORARY_REDIRECT
    );
}

// --- the cart

/// Two sizes of one SKU are two rows. Before each row owned its signals,
/// this panicked: the component repeated without a key.
#[tokio::test]
async fn one_sku_in_two_sizes_is_two_rows() {
    let mut shop = Shop::open().await;
    shop.get("/panier").await.assert_ok("/panier");
    let cart = shop
        .jar
        .iter()
        .find(|(k, _)| k == "cart")
        .map(|(_, v)| v.clone())
        .expect("the shop mints a cart cookie on sight");

    let pool = db::connect(&shop.pool()).await.expect("database");
    sqlx::query("insert into carts (id, created_at) values (?1, datetime('now'))")
        .bind(&cart)
        .execute(&pool)
        .await
        .expect("cart");
    for (sku, size, quantity) in [
        ("COQ-TSHIRT", "M", 2),
        ("COQ-TSHIRT", "L", 1),
        ("COQ-MUG", "", 3),
    ] {
        sqlx::query(
            "insert into cart_lines (cart_id, sku, size, quantity) values (?1, ?2, ?3, ?4)",
        )
        .bind(&cart)
        .bind(sku)
        .bind(size)
        .bind(quantity)
        .execute(&pool)
        .await
        .expect("line");
    }

    let page = shop.get("/panier").await;
    page.assert_ok("/panier");
    assert_eq!(page.count("id=\"line-"), 3, "the cart lost a row");
    // The id is what a morph follows when the list reorders.
    assert!(page.contains("id=\"line-COQ-TSHIRT-M\""));
    assert!(page.contains("id=\"line-COQ-TSHIRT-L\""));
}

#[tokio::test]
async fn an_empty_cart_cannot_be_ordered() {
    let mut shop = Shop::open().await;
    shop.sign_up("panier-vide@bernard.sh").await;

    let page = shop.get("/commander").await;
    page.assert_ok("/commander");
    assert!(page.contains("Rien à commander"));
}

#[tokio::test]
async fn checkout_places_an_order_and_empties_the_cart() {
    let mut shop = Shop::open().await;
    shop.sign_up("acheteur@bernard.sh").await;
    let cart = shop
        .jar
        .iter()
        .find(|(k, _)| k == "cart")
        .map(|(_, v)| v.clone())
        .expect("cart cookie");

    let pool = db::connect(&shop.pool()).await.expect("database");
    sqlx::query("insert or ignore into carts (id, created_at) values (?1, datetime('now'))")
        .bind(&cart)
        .execute(&pool)
        .await
        .expect("cart");
    sqlx::query(
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, 'COQ-TSHIRT', 'M', 1)",
    )
    .bind(&cart)
    .execute(&pool)
    .await
    .expect("line");

    let page = shop
        .post(
            "/commander",
            &[
                ("address", "12 quai de la Douane, 29200 Brest"),
                ("shipping", "standard"),
            ],
        )
        .await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    let reference = page
        .location
        .as_deref()
        .and_then(|l| l.strip_prefix("/commande/"))
        .expect("the order redirects to its tracking page")
        .to_string();

    let tracking = shop.get(&format!("/commande/{reference}")).await;
    tracking.assert_ok("the tracking page");
    for step in ["Payée", "En préparation", "Expédiée", "Livrée"] {
        assert!(tracking.contains(step), "the timeline is missing {step}");
    }

    assert!(
        shop.get("/panier").await.contains("Votre panier est vide"),
        "the order left the cart behind"
    );
}

#[tokio::test]
async fn an_order_belongs_to_the_account_that_placed_it() {
    let mut shop = Shop::open().await;
    shop.sign_up("proprietaire@bernard.sh").await;
    let cart = shop
        .jar
        .iter()
        .find(|(k, _)| k == "cart")
        .map(|(_, v)| v.clone())
        .expect("cart cookie");
    let pool = db::connect(&shop.pool()).await.expect("database");
    sqlx::query("insert or ignore into carts (id, created_at) values (?1, datetime('now'))")
        .bind(&cart)
        .execute(&pool)
        .await
        .expect("cart");
    sqlx::query(
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, 'COQ-MUG', '', 1)",
    )
    .bind(&cart)
    .execute(&pool)
    .await
    .expect("line");
    let reference = shop
        .post(
            "/commander",
            &[
                ("address", "12 quai de la Douane"),
                ("shipping", "standard"),
            ],
        )
        .await
        .location
        .as_deref()
        .and_then(|l| l.strip_prefix("/commande/"))
        .expect("reference")
        .to_string();

    // A second account, on the same shop, must not see it.
    let mut other = Shop::open().await;
    other.database = shop.database.clone();
    other.sign_up("curieux@bernard.sh").await;
    let page = other.get(&format!("/commande/{reference}")).await;
    assert_eq!(
        page.status,
        StatusCode::NOT_FOUND,
        "another account read the order"
    );
    // Both Shops point at one file; let the first one remove it.
    other.database = std::env::temp_dir().join("topcoat-shop-test-unused.db");
}

// --- the back office

#[tokio::test]
async fn the_back_office_is_a_404_for_everyone_else() {
    let mut shop = Shop::open().await;
    for url in [
        "/admin",
        "/admin/produits",
        "/admin/commandes",
        "/admin/clients",
    ] {
        assert_eq!(
            shop.get(url).await.status,
            StatusCode::NOT_FOUND,
            "{url} was reachable"
        );
    }

    // Signed in is not enough: the door does not advertise itself.
    shop.sign_up("simple@bernard.sh").await;
    assert_eq!(shop.get("/admin").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_admin_reaches_the_back_office_and_can_change_a_price() {
    let mut shop = Shop::open().await;
    shop.sign_up("patron@bernard.sh").await;
    let pool = db::connect(&shop.pool()).await.expect("database");
    sqlx::query("update users set admin = 1 where email = 'patron@bernard.sh'")
        .execute(&pool)
        .await
        .expect("raise the flag");

    for url in [
        "/admin",
        "/admin/produits",
        "/admin/nouveau",
        "/admin/commandes",
        "/admin/clients",
        "/admin/produit/COQ-TSHIRT",
    ] {
        shop.get(url).await.assert_ok(url);
    }

    shop.post("/admin/prix", &[("sku", "COQ-MUG"), ("price", "42")])
        .await;
    let cents: i64 = sqlx::query_scalar("select price_cents from products where sku = 'COQ-MUG'")
        .fetch_one(&pool)
        .await
        .expect("price");
    assert_eq!(cents, 4200, "the price did not move");
}

#[tokio::test]
async fn an_order_needs_an_address() {
    let mut shop = Shop::open().await;
    shop.sign_up("sans-adresse@bernard.sh").await;
    let cart = shop
        .jar
        .iter()
        .find(|(k, _)| k == "cart")
        .map(|(_, v)| v.clone())
        .expect("cart cookie");
    let pool = db::connect(&shop.pool()).await.expect("database");
    sqlx::query("insert or ignore into carts (id, created_at) values (?1, datetime('now'))")
        .bind(&cart)
        .execute(&pool)
        .await
        .expect("cart");
    sqlx::query(
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, 'COQ-MUG', '', 1)",
    )
    .bind(&cart)
    .execute(&pool)
    .await
    .expect("line");

    let page = shop
        .post(
            "/commander",
            &[("address", "   "), ("shipping", "standard")],
        )
        .await;

    let placed: i64 = sqlx::query_scalar("select count(*) from orders")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(
        placed, 0,
        "an order went through with no address: {:?}",
        page.location
    );
}

/// The POST answers on the checkout path itself: the form comes back with
/// the reason, and no route exists just to send a 303.
#[tokio::test]
async fn a_refused_order_comes_back_with_the_form() {
    let mut shop = Shop::open().await;
    shop.sign_up("forme@bernard.sh").await;
    let cart = shop
        .jar
        .iter()
        .find(|(k, _)| k == "cart")
        .map(|(_, v)| v.clone())
        .expect("cart cookie");
    let pool = db::connect(&shop.pool()).await.expect("database");
    sqlx::query("insert or ignore into carts (id, created_at) values (?1, datetime('now'))")
        .bind(&cart)
        .execute(&pool)
        .await
        .expect("cart");
    sqlx::query(
        "insert into cart_lines (cart_id, sku, size, quantity) values (?1, 'COQ-MUG', '', 1)",
    )
    .bind(&cart)
    .execute(&pool)
    .await
    .expect("line");

    let page = shop
        .post("/commander", &[("address", ""), ("shipping", "standard")])
        .await;
    page.assert_ok("the refused order");
    assert_eq!(page.location, None, "a refused order redirected");
    assert!(page.contains("Il manque une adresse de livraison."));
    assert!(
        page.contains("Livraison et paiement"),
        "the form did not come back"
    );

    // With an address it goes through, and that one does redirect.
    let page = shop
        .post(
            "/commander",
            &[
                ("address", "12 quai de la Douane"),
                ("shipping", "standard"),
            ],
        )
        .await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert!(
        page.location
            .as_deref()
            .is_some_and(|l| l.starts_with("/commande/"))
    );
}
