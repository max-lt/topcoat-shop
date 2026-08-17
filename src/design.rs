//! The house typefaces. Fontsource resolves them at build time and the
//! asset bundler serves the files from our own origin, so a page needs no
//! third-party request to render as designed.

use topcoat::font::fontsource::fontsource_font;
use topcoat::font::Font;

/// Display face: the serif that carries titles and prices.
pub const SERIF: Font = fontsource_font!(INSTRUMENT_SERIF, weight: 400, host: Asset);

/// Text face: everything else.
pub const SANS: Font = fontsource_font!(INSTRUMENT_SANS, weight: [400, 500, 600], host: Asset);
