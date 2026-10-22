# ADR-0011 - Themes, languages and a fixed logo

Date: 2026-10-22. Status: accepted. Amends BRIEF section 4.

## Context

The V2 mockups (file and session pane) keep the experience and vary the look: ten skins were drawn on
top of the manga style, with the same layout and avatars. The maintainer asked for every one of them,
for the page in several languages, and for a logo that does not change.

## Decision

- **Skins**: the page is styled only through CSS variables (colours, fonts of titles, line widths,
  radii, shadows, background pattern). Twelve skins: manga light and dark, studio pro, cockpit, swiss,
  editorial, forest, neo-brutal, arcade, phosphor, pastel, high contrast. The first visit follows the
  system's light or dark setting; the choice is kept per browser.
- **Two checks in the tests**: no colour, radius or shadow written outside the skin blocks; every listed
  text and background pair reaches a WCAG contrast of 4.5:1 in every skin.
- Body text stays in Atkinson Hyperlegible in every skin; only titles and section bands change font.
- **A skin changes the look, never the meaning**, like an avatar pack (BRIEF section 4). Skin and avatar
  pack are chosen apart; a skin suggests a pack.
- **Languages**: every text of the page comes from a dictionary, English and French first; the browser's
  language by default, a menu to change it. The server answers with error codes the page translates,
  and builds the line the companion mod shows under the prompt in the chosen language.
- **Logo**: one mark and one word, drawn with their own fixed colours on their own tile, the same in
  every skin. Proposal A of the design canvas, chosen on 2026-10-22: a radar echo and the check mark of
  a cleared flight, yellow and white on a near black tile, the word in Dela Gothic One. At 32 and 16 px
  (favicon) the mark drops the radar sweep.
