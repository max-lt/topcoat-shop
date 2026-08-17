//! The pages a real shop owes its visitors: who we are, how to reach us,
//! shipping and returns, sizes, and the legal small print.

use topcoat::router::page;
use topcoat::runtime::Event;
use topcoat::view::view;
use topcoat::Result;

use crate::app::{page_heading, BTN, CARD, EYEBROW, FIELD, MUTED, SOFT};

// --- the house

const CREW: [(&str, &str, &str); 3] = [
    ("BH", "Bernard L'Hermite", "Fondateur, choisit les coquilles"),
    ("CF", "Coco Fiddler", "Matières et ateliers"),
    ("PP", "Pat Pagure", "Commandes et colis"),
];

#[page("/maison")]
async fn house() -> Result {
    view! {
        <div class="max-w-3xl">
            page_heading(
                eyebrow: "La maison",
                title: "La plus petite coquille qui vous abrite",
                lede: ""
            )

            <div class=("mt-10 space-y-6 text-lg leading-relaxed ".to_string() + SOFT)>
                <p>
                    "Bernard héberge des applications dans des images de quelques mégaoctets. \
                     Un jour, l'équipe a voulu un sweat avec le crabe brodé dans le dos ; \
                     dix-huit personnes l'ont demandé ensuite, et la boutique est née."
                </p>
                <p>
                    "Nous appliquons aux objets la règle que nous appliquons au logiciel : \
                     ce qui n'est pas nécessaire n'est pas ajouté. Peu de pièces, des \
                     matières qu'on peut nommer, des séries courtes, et des stocks affichés \
                     tels qu'ils sont."
                </p>
                <p>
                    "Tout est dessiné à Brest, au bord de l'eau, et fabriqué au plus près : \
                     Gard pour le coton, Portugal pour le molleton, Bretagne pour le papier."
                </p>
            </div>
        </div>

        <div class="mt-14 max-w-4xl">
            <div class="aspect-video overflow-hidden rounded-3xl bg-oat-100 ring-1 ring-oat-200"
                 data-bg=(crate::images::background("maison-rade"))>
                <img src=(crate::images::url("maison-rade", 900))
                     srcset=(format!("{} 900w, {} 1600w", crate::images::url("maison-rade", 900), crate::images::url("maison-rade", 1600)))
                     sizes="(min-width: 1024px) 56rem, 100vw"
                     alt="Un phare au large de la rade, sur son îlot rocheux"
                     loading="lazy"
                     class="h-full w-full object-cover">
            </div>
            <p class=("mt-3 text-center text-sm ".to_string() + MUTED)>"La rade, vue du bureau — les jours où le bureau est un ponton."</p>
        </div>

        <section class="mt-20">
            <h2 class="text-3xl">"L'équipage"</h2>
            <div class="mt-10 grid max-w-4xl gap-6 sm:grid-cols-3">
                for (initials, name, role) in CREW {
                    <div class=(CARD.to_string() + " p-8 text-center")>
                        <span class="mx-auto flex h-16 w-16 items-center justify-center rounded-full bg-gin-100 font-display text-xl text-gin-800">(initials)</span>
                        <p class="mt-4 font-medium">(name)</p>
                        <p class=("mt-1 text-sm ".to_string() + MUTED)>(role)</p>
                    </div>
                }
            </div>
        </section>
    }
}

// --- help

const QUESTIONS: [(&str, &str); 6] = [
    ("Quels sont les délais de livraison ?",
     "Standard : 3 à 5 jours ouvrés, 4,90 €. Express : 24 à 48 heures, 11,90 €. \
      Au-delà de 50 € d'achat, la livraison est offerte quel que soit le mode choisi."),
    ("Puis-je retourner un article ?",
     "Trente jours à compter de la réception, article non porté et étiquette d'origine. \
      L'échange de taille est gratuit ; le remboursement se fait sous cinq jours après \
      réception du retour."),
    ("Comment choisir ma taille ?",
     "Nos vêtements taillent grand d'environ une demi-taille. Le t-shirt en S mesure \
      48 cm d'épaule à épaule, puis 2 cm par taille. En cas d'hésitation, prenez la \
      taille en dessous de votre habitude."),
    ("Pourquoi certaines tailles sont-elles épuisées ?",
     "Nous produisons par séries de deux cents pièces et n'affichons que le stock réel. \
      Une taille manquante revient à la série suivante, en général sous six semaines."),
    ("Livrez-vous à l'étranger ?",
     "Pour l'instant, France métropolitaine et Belgique. Écrivez-nous si vous êtes \
      ailleurs : nous étudions les envois au cas par cas."),
    ("Mes données sont-elles conservées ?",
     "Votre email, votre nom et vos commandes, le temps de la garantie légale. Le mot \
      de passe n'est jamais stocké en clair : la base ne contient qu'une empreinte \
      argon2id, inutilisable pour se connecter."),
];

const SIZES: [(&str, &str, &str, &str); 4] = [
    ("S", "48 cm", "68 cm", "36-39"),
    ("M", "50 cm", "70 cm", "40-43"),
    ("L", "52 cm", "72 cm", "44-46"),
    ("XL", "54 cm", "74 cm", "—"),
];

#[page("/aide")]
async fn help() -> Result {
    view! {
        <div class="max-w-3xl">
            page_heading(
                eyebrow: "Aide",
                title: "Livraison, retours et tailles",
                lede: "Tout ce qu'il faut savoir avant de commander. Si la réponse n'est \
                       pas là, écrivez-nous : un humain répond sous 24 heures."
            )

            <section class="mt-14" id="tailles">
                <h2 class="text-3xl">"Guide des tailles"</h2>
                <div class="mt-6 overflow-x-auto">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="border-b border-oat-300 text-left">
                                <th class="py-3 font-medium">"Taille"</th>
                                <th class="py-3 font-medium">"Épaules"</th>
                                <th class="py-3 font-medium">"Longueur"</th>
                                <th class="py-3 font-medium">"Chaussettes"</th>
                            </tr>
                        </thead>
                        <tbody>
                            for (size, shoulders, length, shoe) in SIZES {
                                <tr class="border-b border-oat-200">
                                    <td class="py-3 font-medium">(size)</td>
                                    <td class=("py-3 tabular-nums ".to_string() + MUTED)>(shoulders)</td>
                                    <td class=("py-3 tabular-nums ".to_string() + MUTED)>(length)</td>
                                    <td class=("py-3 tabular-nums ".to_string() + MUTED)>(shoe)</td>
                                </tr>
                            }
                        </tbody>
                    </table>
                </div>
            </section>

            <section class="mt-16">
                <h2 class="text-3xl">"Questions fréquentes"</h2>
                <div class="mt-6">
                    for (question, answer) in QUESTIONS {
                        // The padding belongs to the summary, not the details:
                        // it is the summary that takes the click.
                        <details class="group border-b border-oat-200">
                            <summary class="flex cursor-pointer items-center justify-between gap-4 py-5 font-medium transition hover:text-gin-700">
                                (question)
                                <span class="shrink-0 text-oat-500 transition group-open:rotate-45">"+"</span>
                            </summary>
                            <p class=("-mt-1 pb-5 leading-relaxed ".to_string() + SOFT)>(answer)</p>
                        </details>
                    }
                </div>
            </section>
        </div>
    }
}

// --- contact

#[page("/contact")]
async fn contact() -> Result {
    view! {
        signal name = String::new();
        signal email = String::new();
        signal message = String::new();
        signal sent = false;
        signal blank = String::new();

        <div class="grid gap-14 lg:grid-cols-2">
            <div>
                page_heading(
                    eyebrow: "Contact",
                    title: "Écrivez-nous",
                    lede: "Une question de taille, une commande à modifier, ou juste envie \
                           de dire bonjour au crabe."
                )
                <dl class="mt-10 space-y-5 text-sm">
                    <div>
                        <dt class="font-medium">"Email"</dt>
                        <dd class=("mt-1 ".to_string() + MUTED)>"bonjour@bernard.sh"</dd>
                    </div>
                    <div>
                        <dt class="font-medium">"Atelier"</dt>
                        <dd class=("mt-1 leading-relaxed ".to_string() + MUTED)>"12 quai de la Douane" <br> "29200 Brest"</dd>
                    </div>
                    <div>
                        <dt class="font-medium">"Délai de réponse"</dt>
                        <dd class=("mt-1 ".to_string() + MUTED)>"Un humain sous 24 h, un crabe sous 48 h."</dd>
                    </div>
                </dl>
            </div>

            <div class=(CARD.to_string() + " p-8")>
                <div :hidden=$(sent.get())>
                    <div class="space-y-5">
                        <div>
                            <label class="text-sm font-medium">"Nom"</label>
                            <input class=(FIELD.to_string() + " mt-2") placeholder="Marie Carapace"
                                   :value=$(name.get()) @input=$(|e: Event| name.set(e.target.value))>
                        </div>
                        <div>
                            <label class="text-sm font-medium">"Email"</label>
                            <input class=(FIELD.to_string() + " mt-2") placeholder="marie@coquille.fr"
                                   :value=$(email.get()) @input=$(|e: Event| email.set(e.target.value))>
                            <p class="mt-2 text-xs text-brique-700"
                               :hidden=$(if email.get().trim().is_empty() { true } else { if email.get().contains("@") { email.get().contains(".") } else { false } })>
                                "Cette adresse ne ressemble pas à une adresse."
                            </p>
                        </div>
                        <div>
                            <label class="text-sm font-medium">"Message"</label>
                            <textarea class=(FIELD.to_string() + " mt-2") rows="5" placeholder="Votre message…"
                                      :value=$(message.get()) @input=$(|e: Event| message.set(e.target.value))></textarea>
                        </div>
                        <button class=(BTN.to_string() + " w-full")
                                :disabled=$(if name.get().trim().is_empty() { true } else { if message.get().trim().is_empty() { true } else { !email.get().contains("@") } })
                                @click=$(|_e| {
                                    sent.set(true);
                                    message.set(blank.get());
                                })>"Envoyer"</button>
                    </div>
                </div>

                <div class="py-12 text-center" :hidden=$(!sent.get())>
                    <p class="text-4xl">"✓"</p>
                    <p class="mt-4 font-display text-2xl">"Merci " $(name.get()) " !"</p>
                    <p class=("mt-2 text-sm ".to_string() + SOFT)>
                        "Votre message est arrivé dans la coquille. On vous répond très vite."
                    </p>
                </div>
            </div>
        </div>
    }
}

// --- legal

#[page("/cgv")]
async fn terms() -> Result {
    let articles = [
        ("Objet", "Les présentes conditions régissent les ventes conclues sur bernard.sh. \
                   Cette boutique est une démonstration : aucune commande n'est expédiée et \
                   aucun paiement n'est encaissé."),
        ("Prix", "Les prix sont indiqués en euros, toutes taxes comprises, hors frais de \
                  livraison. Le prix retenu est celui affiché au moment de la validation."),
        ("Commande", "La commande est ferme à la validation du panier. Un récapitulatif est \
                      consultable dans l'espace client, avec le suivi de son acheminement."),
        ("Livraison", "France métropolitaine et Belgique. Standard 3 à 5 jours ouvrés, \
                       express 24 à 48 heures, offerte au-delà de 50 € d'achat."),
        ("Rétractation", "Trente jours à compter de la réception, sans avoir à se justifier. \
                          Les frais de retour restent à la charge de l'acheteur, sauf erreur \
                          de notre part."),
        ("Données", "Les données collectées servent au traitement des commandes et ne sont \
                     transmises à aucun tiers. Le mot de passe n'est conservé que sous forme \
                     d'empreinte argon2id."),
    ];

    view! {
        <div class="max-w-2xl">
            page_heading(eyebrow: "Légal", title: "Conditions générales de vente", lede: "")
            <div class="mt-12 space-y-10">
                for (i, (title, text)) in articles.iter().enumerate() {
                    <section>
                        <h2 class="text-xl">(format!("Article {} — {title}", i + 1))</h2>
                        <p class=("mt-3 leading-relaxed ".to_string() + SOFT)>(*text)</p>
                    </section>
                }
            </div>
        </div>
    }
}

#[page("/mentions-legales")]
async fn legal() -> Result {
    view! {
        <div class="max-w-2xl">
            page_heading(eyebrow: "Légal", title: "Mentions légales", lede: "")
            <dl class=("mt-12 space-y-8 leading-relaxed ".to_string() + SOFT)>
                <div>
                    <dt class="font-medium text-oat-900">"Éditeur"</dt>
                    <dd class="mt-2">"Bernard SAS, 12 quai de la Douane, 29200 Brest. \
                                      Société fictive, créée pour une démonstration technique."</dd>
                </div>
                <div>
                    <dt class="font-medium text-oat-900">"Hébergement"</dt>
                    <dd class="mt-2">"Cette boutique tourne sur un serveur Rust bâti avec \
                                      Topcoat, servi depuis une image minimale."</dd>
                </div>
                <div>
                    <dt class="font-medium text-oat-900">"Propriété intellectuelle"</dt>
                    <dd class="mt-2">"Les textes et visuels de ce site sont produits pour la \
                                      démonstration. Les fontes Instrument Serif et Instrument \
                                      Sans sont distribuées sous licence libre."</dd>
                </div>
                <div>
                    <dt class="font-medium text-oat-900">"Photographies"</dt>
                    <dd class="mt-2">
                        "Les photographies des produits proviennent de "
                        <a href="https://unsplash.com" class="underline underline-offset-4">"Unsplash"</a>
                        ", où leurs auteurs les publient sous la licence Unsplash. \
                         Elles sont redimensionnées à la volée par le serveur."
                    </dd>
                </div>
                <div>
                    <dt class="font-medium text-oat-900">"Contact"</dt>
                    <dd class="mt-2">"bonjour@bernard.sh"</dd>
                </div>
            </dl>
            <p class=("mt-12 text-sm ".to_string() + EYEBROW)>"Boutique de démonstration"</p>
        </div>
    }
}
