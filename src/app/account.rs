//! Accounts: register, sign in, sign out, and the order history. The forms
//! are plain POSTs so the whole flow works with JavaScript switched off --
//! the session cookie has to be set on a response either way.

use topcoat::context::Cx;
use topcoat::router::error::{redirect, see_other, RouterErrorExt, SeeOther};
use topcoat::router::{content::Form, page, query_params, route};
use topcoat::session;
use topcoat::view::view;
use topcoat::Result;

use crate::app::context::{current_cart, current_user, pool};
use crate::app::{page_heading, BTN, BTN_OUTLINE, CARD, EYEBROW, FIELD, MUTED};
use crate::db;

#[query_params(error = bad_request)]
struct Message {
    err: Option<String>,
}

#[page("/connexion")]
async fn sign_in_page(cx: &Cx) -> Result {
    if current_user(cx).await?.is_some() {
        return Err(redirect("/compte").into());
    }
    let message = query_params::<Message>(cx)?.err.clone().unwrap_or_default();

    view! {
        <div class="mx-auto max-w-4xl">
            page_heading(
                eyebrow: "Compte",
                title: "Se connecter",
                lede: "Un compte sert à suivre vos commandes et à retrouver vos adresses. \
                       Le mot de passe est haché en argon2id\u{202f}: la base ne contient rien \
                       qui puisse être rejoué."
            )

            if !message.is_empty() {
                <p class="mt-8 rounded-2xl bg-brique-100 px-5 py-4 text-sm text-brique-700">(message)</p>
            }

            <div class="mt-10 grid gap-6 lg:grid-cols-2">
                <form method="post" action="/connexion" class=(CARD.to_string() + " space-y-5 p-8")>
                    <h2 class="text-2xl">"J'ai déjà un compte"</h2>
                    <div>
                        <label class="text-sm font-medium">"Email"</label>
                        <input class=(FIELD.to_string() + " mt-2") type="email" name="email" required="required" placeholder="marie@coquille.fr">
                    </div>
                    <div>
                        <label class="text-sm font-medium">"Mot de passe"</label>
                        <input class=(FIELD.to_string() + " mt-2") type="password" name="password" required="required">
                    </div>
                    <button class=(BTN.to_string() + " w-full")>"Se connecter"</button>
                </form>

                <form method="post" action="/inscription" class=(CARD.to_string() + " space-y-5 p-8")>
                    <h2 class="text-2xl">"Je crée un compte"</h2>
                    <div>
                        <label class="text-sm font-medium">"Nom"</label>
                        <input class=(FIELD.to_string() + " mt-2") name="name" required="required" placeholder="Marie Carapace">
                    </div>
                    <div>
                        <label class="text-sm font-medium">"Email"</label>
                        <input class=(FIELD.to_string() + " mt-2") type="email" name="email" required="required" placeholder="marie@coquille.fr">
                    </div>
                    <div>
                        <label class="text-sm font-medium">"Mot de passe"</label>
                        <input class=(FIELD.to_string() + " mt-2") type="password" name="password" required="required" minlength="8">
                        <p class=("mt-2 text-xs ".to_string() + MUTED)>"Huit caractères au moins."</p>
                    </div>
                    <button class=(BTN.to_string() + " w-full")>"Créer mon compte"</button>
                </form>
            </div>
        </div>
    }
}

#[derive(serde::Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct Registration {
    name: String,
    email: String,
    password: String,
}

/// Signing in mints a fresh token (Topcoat's side) and records its hash
/// (ours), then claims whatever the anonymous cart held.
#[route(POST "/connexion")]
async fn sign_in(cx: &Cx, Form(f): Form<Credentials>) -> Result<SeeOther> {
    let Some(user) = db::verify_credentials(pool(cx), &f.email, &f.password).await? else {
        return Ok(see_other("/connexion?err=Identifiants+incorrects."));
    };
    start_session(cx, user.id).await?;
    Ok(see_other("/compte"))
}

#[route(POST "/inscription")]
async fn register(cx: &Cx, Form(f): Form<Registration>) -> Result<SeeOther> {
    if f.password.len() < 8 {
        return Ok(see_other("/connexion?err=Mot+de+passe+trop+court."));
    }
    if db::email_taken(pool(cx), &f.email).await? {
        return Ok(see_other("/connexion?err=Cet+email+a+déjà+un+compte."));
    }
    let user = db::register(pool(cx), &f.email, &f.name, &f.password).await?;
    start_session(cx, user.id).await?;
    Ok(see_other("/compte"))
}

async fn start_session(cx: &Cx, user_id: i64) -> Result<()> {
    let session = session::start(cx).await?;
    let left = session
        .expires_at
        .duration_since(std::time::SystemTime::now())
        .unwrap_or(std::time::Duration::from_secs(60 * 60 * 24 * 30));
    let expires_at = chrono::Utc::now()
        + chrono::Duration::from_std(left).unwrap_or_else(|_| chrono::Duration::days(30));

    db::open_session(pool(cx), session.token_hash.as_ref(), user_id, expires_at).await?;

    // The cart the visitor filled before signing in is now theirs.
    let cart = current_cart(cx);
    db::attach_cart(pool(cx), &cart, user_id).await?;
    Ok(())
}

#[route(POST "/deconnexion")]
async fn sign_out(cx: &Cx) -> Result<SeeOther> {
    if let Some(hash) = session::stop(cx).await? {
        db::close_session(pool(cx), hash.as_ref()).await?;
    }
    Ok(see_other("/"))
}

#[page("/compte")]
async fn account(cx: &Cx) -> Result {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;

    view! {
        <div class="flex flex-wrap items-end justify-between gap-6">
            <div>
                <p class=(EYEBROW)>"Compte"</p>
                <h1 class="mt-3 text-4xl sm:text-5xl">(&user.name)</h1>
                <p class=("mt-2 ".to_string() + MUTED)>(&user.email)</p>
            </div>
            <form method="post" action="/deconnexion">
                <button class=(BTN_OUTLINE)>"Se déconnecter"</button>
            </form>
        </div>
    }
}
