//! The journal: what the house chooses, and why. Editorial content sells
//! objects better than adjectives do.

use topcoat::context::Cx;
use topcoat::router::error::RouterErrorExt;
use topcoat::router::{page, path_param};
use topcoat::view::view;
use topcoat::Result;

use crate::app::{page_heading, BTN_OUTLINE, EYEBROW, MUTED, SOFT};

pub const POSTS: [(&str, &str, &str, &str, &str); 3] = [
    (
        "le-coton-quon-peut-nommer",
        "12 août 2026",
        "Matières",
        "Le coton qu'on peut nommer",
        "Un t-shirt ne coûte pas 29 € par hasard. Voilà d'où vient la fibre, ce que \
         l'atelier facture, et pourquoi nous imprimons à l'encre à l'eau.",
    ),
    (
        "petite-serie-grand-soin",
        "2 août 2026",
        "Atelier",
        "Petite série, grand soin",
        "Nous produisons par lots de deux cents pièces. C'est peu, c'est plus cher à \
         l'unité, et c'est la seule façon de ne jamais brûler d'invendus.",
    ),
    (
        "pourquoi-un-crabe",
        "20 juillet 2026",
        "Maison",
        "Pourquoi un crabe",
        "Le bernard-l'hermite ne construit pas sa coquille : il choisit la plus petite \
         qui l'abrite en entier. Toute la maison tient dans cette phrase.",
    ),
];

const BADGE: &str = "rounded-full bg-gin-100 px-3 py-1 text-xs font-medium text-gin-800";

/// The tag is the one field the listing and the article share, so it also
/// picks the photo.
pub fn photo_key(tag: &str) -> &'static str {
    match tag {
        "Matières" => "journal-coton",
        "Atelier" => "journal-atelier",
        _ => "journal-crabe",
    }
}

#[page("/journal")]
async fn journal() -> Result {
    view! {
        page_heading(
            eyebrow: "Journal",
            title: "Ce qu'on choisit, et pourquoi",
            lede: "Les matières, l'atelier, les décisions qu'on assume. Écrit par les gens \
                   qui passent les commandes."
        )

        <div class="mt-14 grid gap-10 lg:grid-cols-3">
            for (slug, date, tag, title, lede) in POSTS {
                <a href=("/journal/".to_string() + slug) class="group block">
                    <div class="aspect-video overflow-hidden rounded-2xl bg-oat-100 ring-1 ring-oat-200 transition group-hover:ring-gin-300"
                         data-vt=(slug)
                         data-bg=(crate::images::background(photo_key(tag)))>
                        <img src=(crate::images::url(photo_key(tag), 400))
                             srcset=(format!("{} 400w, {} 900w", crate::images::url(photo_key(tag), 400), crate::images::url(photo_key(tag), 900)))
                             sizes="(min-width: 1024px) 33vw, 100vw"
                             alt=(title)
                             loading="lazy"
                             class="h-full w-full object-cover transition duration-500 group-hover:scale-105">
                    </div>
                    <div class="mt-5 flex items-center gap-3">
                        <span class=(BADGE)>(tag)</span>
                        <time class=("text-sm ".to_string() + MUTED)>(date)</time>
                    </div>
                    <h2 class="mt-3 text-2xl leading-snug">(title)</h2>
                    <p class=("mt-2 leading-relaxed ".to_string() + SOFT)>(lede)</p>
                </a>
            }
        </div>
    }
}

#[path_param]
struct Slug(str);

#[page("/journal/{slug}")]
async fn post(cx: &Cx) -> Result {
    let slug = path_param::<Slug>(cx).to_string();
    let (_, date, tag, title, lede) = POSTS.iter().find(|(s, ..)| *s == slug).ok_or_not_found()?;
    let body = body(&slug).ok_or_not_found()?;

    view! {
        <article class="mx-auto max-w-2xl">
            <div class="flex items-center gap-3">
                <span class=(BADGE)>(tag)</span>
                <time class=("text-sm ".to_string() + MUTED)>(date)</time>
            </div>
            <h1 class="mt-5 text-4xl leading-tight sm:text-5xl">(title)</h1>
            <p class=("mt-5 text-lg leading-relaxed ".to_string() + SOFT)>(lede)</p>

            <div class="relative mt-8 aspect-video overflow-hidden rounded-3xl bg-oat-100 ring-1 ring-oat-200"
                 data-vt=(&slug)
                 data-bg=(crate::images::background(photo_key(tag)))>
                // Same bridge as the product hero: the listing's 400 px tile
                // sits blurred underneath while the big one travels.
                <img src=(crate::images::url(photo_key(tag), 400))
                     alt=""
                     aria-hidden="true"
                     class="absolute inset-0 h-full w-full scale-105 object-cover blur-sm">
                <img src=(crate::images::url(photo_key(tag), 900))
                     srcset=(format!("{} 900w, {} 1600w", crate::images::url(photo_key(tag), 900), crate::images::url(photo_key(tag), 1600)))
                     sizes="(min-width: 768px) 42rem, 100vw"
                     alt=(title)
                     fetchpriority="high"
                     class="relative h-full w-full object-cover">
            </div>

            <div class="mt-10 space-y-6">
                for (rank, paragraph) in body.iter().enumerate() {
                    // The drop cap's top should sit on the first line's cap
                    // height; the serif ascends past its box, so nudge down.
                    <p class=(if rank == 0 {
                        "leading-[1.8] text-oat-700 first-letter:float-left first-letter:mt-2 first-letter:pr-3 first-letter:font-display first-letter:text-6xl first-letter:leading-[0.75] first-letter:text-gin-800"
                    } else {
                        "leading-[1.8] text-oat-700"
                    })>(*paragraph)</p>
                }
            </div>

            <div class="mt-14 rounded-2xl bg-oat-100 px-6 py-8 text-center ring-1 ring-oat-200">
                <p class=(EYEBROW)>"La collection"</p>
                <p class="mt-3 font-display text-2xl">"Voir les pièces dont on parle"</p>
                <a href="/boutique" class=(BTN_OUTLINE.to_string() + " mt-6")>"Ouvrir la boutique"</a>
            </div>

            <p class="mt-12">
                <a href="/journal" class="text-sm text-gin-700 underline underline-offset-4">"← Tous les billets"</a>
            </p>
        </article>
    }
}

fn body(slug: &str) -> Option<Vec<&'static str>> {
    Some(match slug {
        "le-coton-quon-peut-nommer" => vec![
            "La fibre vient d'une coopérative du Gard qui cultive en biologique depuis 2011. \
             Nous ne disons pas « coton premium » : nous disons 180 grammes au mètre carré, \
             filé et tricoté en Europe, avec une facture que nous pouvons montrer.",
            "L'impression se fait à l'encre à l'eau, plus pâle au premier lavage que le \
             plastisol dont se servent la plupart des ateliers. C'est un défaut assumé : \
             l'encre plastique tient mieux, mais elle craquelle en trois ans et ne part \
             jamais vraiment de l'eau de rinçage.",
            "Reste le prix. Vingt-neuf euros, dont un peu moins de la moitié part dans la \
             matière et la confection. Nous préférons l'écrire plutôt que de laisser croire \
             à une marge de luxe : sur une série de deux cents pièces, il n'y a pas de \
             volume qui fasse baisser quoi que ce soit.",
        ],
        "petite-serie-grand-soin" => vec![
            "Deux cents pièces, c'est le minimum qu'un atelier accepte sans facturer une \
             pénalité de réglage. C'est aussi, à peu près, ce que nous écoulons en une \
             saison. Le calcul est simple : produire ce que l'on vend, plutôt que vendre \
             ce que l'on a produit.",
            "L'inconvénient est visible sur cette boutique — des tailles manquent, parfois \
             des semaines. Nous laissons le stock réel à l'écran plutôt que d'afficher une \
             disponibilité optimiste : voir « épuisé » est désagréable, recevoir un mail \
             d'annulation trois jours après l'est davantage.",
            "L'avantage ne se voit pas du tout : aucune benne. Les maisons qui soldent à \
             moins soixante-dix pour cent ont fabriqué trop, et l'écart finit quelque part. \
             Chez nous il finit dans la série suivante, avec une couleur en moins.",
        ],
        "pourquoi-un-crabe" => vec![
            "Bernard héberge des applications dans des images minuscules : le strict \
             nécessaire pour tourner, rien de plus. Le bernard-l'hermite faisait un emblème \
             évident — il ne construit pas sa coquille, il choisit la plus petite qui \
             l'abrite en entier, et il en change quand il grandit.",
            "La boutique est née d'une blague d'atelier : un sweat pour l'équipe, avec le \
             crabe brodé dans le dos. Il a fallu dix-huit demandes extérieures pour que \
             nous acceptions d'en faire une vraie série, et deux de plus pour que nous \
             ajoutions les chaussettes.",
            "La règle est restée celle de nos images : ce qui n'est pas nécessaire n'est pas \
             ajouté. Pas de collection capsule tous les deux mois, pas de logo sur toute la \
             poitrine. Une pièce entre au catalogue quand quelqu'un de la maison la porte \
             déjà depuis six mois.",
        ],
        _ => return None,
    })
}
