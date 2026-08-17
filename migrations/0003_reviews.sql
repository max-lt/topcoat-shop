-- Seeded customer reviews, so a fresh install has something to show.

insert into reviews (sku, author, rating, text, created_at) values
  ('COQ-HOODIE', 'Solène M.', 5, 'Le molleton est lourd, les finitions impeccables. Le crabe dans le dos fait son petit effet en réunion.', '2026-06-28T09:05:00'),
  ('COQ-TSHIRT', 'Camille R.', 5, 'Le coton est dense sans être raide, et la coupe tombe exactement comme sur la photo. Lavé six fois, l''encre n''a pas bougé.', '2026-07-02T10:14:00'),
  ('COQ-CARNET', 'Hugo V.', 5, 'Le papier pointillé prend l''encre sans baver. Un carnet entier de post-mortems, aucun regret.', '2026-07-08T16:55:00'),
  ('COQ-MUG', 'Inès B.', 5, 'L''émail est magnifique, la contenance parfaite pour une revue de code entière, comme promis.', '2026-07-11T08:47:00'),
  ('COQ-THEIERE', 'Étienne G.', 4, 'Belle fonte, le filtre est fin. Un litre, c''est en réalité trois mugs 4 ms — comptez large.', '2026-07-15T11:30:00'),
  ('COQ-TSHIRT', 'Yann K.', 4, 'Très beau blanc, col qui tient. Une demi-taille en dessous de d''habitude, le conseil du guide est juste.', '2026-07-19T18:40:00'),
  ('COQ-CHAUSSETTES', 'Marc D.', 5, 'Fil d''Écosse sérieux, motif discret. Portées le jour d''un déploiement : zéro incident. Corrélation n''est pas causalité, mais je recommande.', '2026-07-25T14:33:00'),
  ('COQ-HOODIE', 'Adrien T.', 4, 'Chaud et bien coupé. J''enlève une étoile parce que ma taille a mis cinq semaines à revenir — mais au moins le stock affiché était honnête.', '2026-07-30T21:12:00'),
  ('COQ-GOURDE', 'Léa P.', 4, 'Garde le café chaud toute la matinée. Le vert olive est plus profond qu''à l''écran, tant mieux.', '2026-08-01T12:20:00'),
  ('COQ-PLAID', 'Margaux L.', 5, 'Grand, lourd, exactement la couleur du site. Le chat l''a annexé en dix minutes.', '2026-08-06T19:02:00'),
  ('COQ-MARQUEPAGE', 'Nora S.', 5, 'Le laiton patine joliment. Cadeau parfait pour les gens qui cornent les pages, qu''ils arrêtent.', '2026-08-09T15:41:00'),
  ('COQ-TOTE', 'Basile F.', 4, 'Toile épaisse, anses solides, il avale un marché entier. Il manque juste une poche intérieure.', '2026-08-10T10:08:00'),
  ('COQ-TSHIRT', 'Lechat M.', 5, 'trop propre !', '2026-08-16T21:33:51.604020+00:00');
