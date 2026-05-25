# Test fixtures: fonts

Sample fonts used by the font-viewer tests and for manual smoke-testing
the specimen render pipeline. Three faces with deliberately distinct
styles so the ASCII output is visually distinguishable:

| Directory   | Source             | License | Notes                                                  |
|-------------|--------------------|---------|--------------------------------------------------------|
| `sacramento/` | Google Fonts (Astigmatic / Brian J. Bonislawsky) | OFL 1.1 | Thin, formal calligraphy script — long swooshes, low contrast |
| `greatvibes/` | Google Fonts (TypeSETit) | OFL 1.1 | Classic engraver script — pronounced thin/thick contrast       |
| `cabin/`      | Google Fonts (Pablo Impallari)             | OFL 1.1 | Modern grotesque sans — clean baseline for comparison           |

Each subdirectory carries the upstream `OFL.txt` license alongside the
font binary. The fonts are redistributed verbatim from `google/fonts`
on GitHub; no modifications, no Reserved Font Name violations.

These fixtures are not bundled into the `peek` release binary — they
live under `test-data/` so the published crate stays small and they
ship only with the source checkout.
