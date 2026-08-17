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
use crate::app::orders::status_badge;
use crate::app::{page_heading, BTN, BTN_OUTLINE, CARD, EYEBROW, FIELD, MUTED, SOFT};
use crate::db::{self, format_price};

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
    let orders = db::orders(pool(cx), user.id).await?;
    let addresses = db::addresses(pool(cx), user.id).await?;
    let no_orders = orders.is_empty();
    let how_many = orders.len();
    let no_address = addresses.is_empty();

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

        <section class="mt-16" id="adresses">
            <h2 class="text-3xl">"Vos adresses"</h2>

            if no_address {
                <p class=("mt-4 text-sm ".to_string() + MUTED)>
                    "Aucune adresse enregistrée — la première se propose au moment de commander."
                </p>
            } else {
                <ul class="mt-6 grid gap-4 sm:grid-cols-2">
                    for a in &addresses {
                        <li class=(CARD.to_string() + " p-5")>
                            <div class="flex items-baseline justify-between gap-3">
                                <span class="font-medium">(&a.label)</span>
                                if a.is_default != 0 {
                                    <span class="rounded-full bg-gin-100 px-2.5 py-0.5 text-xs font-medium text-gin-800">"Par défaut"</span>
                                }
                            </div>
                            <p class=("mt-2 whitespace-pre-line text-sm ".to_string() + SOFT)>(&a.text)</p>
                            <div class="mt-4 flex gap-4 text-sm">
                                if a.is_default == 0 {
                                    <form method="post" action="/adresses/defaut">
                                        <input type="hidden" name="id" value=(a.id)>
                                        <button class="underline underline-offset-4 transition hover:text-gin-700">"Par défaut"</button>
                                    </form>
                                }
                                <form method="post" action="/adresses/supprimer">
                                    <input type="hidden" name="id" value=(a.id)>
                                    <button class=("underline underline-offset-4 transition hover:text-brique-700 ".to_string() + MUTED)>"Supprimer"</button>
                                </form>
                            </div>
                        </li>
                    }
                </ul>
            }

            <details class="mt-6">
                <summary class=("text-sm underline underline-offset-4 ".to_string() + MUTED)>"Ajouter une adresse"</summary>
                <form method="post" action="/adresses" class=(CARD.to_string() + " mt-4 max-w-xl space-y-4 p-6")>
                    <div>
                        <label class="text-sm font-medium">"Libellé"</label>
                        <input class=(FIELD.to_string() + " mt-2") name="label" required="required" placeholder="Chez moi, Bureau…">
                    </div>
                    <div>
                        <label class="text-sm font-medium">"Adresse"</label>
                        <textarea class=(FIELD.to_string() + " mt-2") name="text" required="required" rows="3" placeholder="12 rue de la Marée&#10;29200 Brest"></textarea>
                    </div>
                    <button class=(BTN_OUTLINE)>"Enregistrer"</button>
                </form>
            </details>
        </section>

        <section class="mt-16">
            <div class="flex items-baseline justify-between">
                <h2 class="text-3xl">"Vos commandes"</h2>
                if !no_orders {
                    <span class=("text-sm ".to_string() + MUTED)>
                        (format!("{how_many} commande{}", if how_many > 1 { "s" } else { "" }))
                    </span>
                }
            </div>

            if no_orders {
                <div class=(CARD.to_string() + " mt-8 p-16 text-center")>
                    <p class=(SOFT)>"Aucune commande pour l'instant."</p>
                    <a href="/boutique" class=(BTN.to_string() + " mt-6")>"Voir la collection"</a>
                </div>
            } else {
                <ul class="mt-8 divide-y divide-oat-200 border-y border-oat-200">
                    for o in orders {
                        <a href=("/commande/".to_string() + &o.reference) class="block transition hover:bg-oat-100">
                            <li class="flex flex-wrap items-center gap-4 px-2 py-5">
                                <span class="font-medium tabular-nums">(&o.reference)</span>
                                status_badge(status: o.status.clone())
                                <span class=("text-sm ".to_string() + MUTED)>(o.created_at.get(..10).unwrap_or_default().to_string())</span>
                                <span class="ml-auto tabular-nums">(format_price(o.total_cents))</span>
                                <span class="text-gin-700">"→"</span>
                            </li>
                        </a>
                    }
                </ul>
            }
        </section>
    }
}

// --- address book

#[derive(serde::Deserialize)]
struct NewAddress {
    label: String,
    text: String,
}

#[route(POST "/adresses")]
async fn add_address(cx: &Cx, Form(f): Form<NewAddress>) -> Result<SeeOther> {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    db::add_address(pool(cx), user.id, &f.label, &f.text).await?;
    Ok(see_other("/compte#adresses"))
}

#[derive(serde::Deserialize)]
struct AddressTarget {
    id: i64,
}

#[route(POST "/adresses/supprimer")]
async fn remove_address(cx: &Cx, Form(f): Form<AddressTarget>) -> Result<SeeOther> {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    db::remove_address(pool(cx), user.id, f.id).await?;
    Ok(see_other("/compte#adresses"))
}

#[route(POST "/adresses/defaut")]
async fn default_address(cx: &Cx, Form(f): Form<AddressTarget>) -> Result<SeeOther> {
    let user = current_user(cx).await?.ok_or_redirect("/connexion")?;
    db::set_default_address(pool(cx), user.id, f.id).await?;
    Ok(see_other("/compte#adresses"))
}
