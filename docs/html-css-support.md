# HTML/CSS support in `perfect-print-html`

`perfect-print-html` renders a deliberately constrained HTML/CSS subset to
the canonical `DocumentModel` — no browser, no WebView. `HtmlDocument` →
parse with `scraper`/`html5ever` → resolve a CSS subset (inline `style=`,
`<style>` blocks with tag/class/id selectors, cascade with inheritance) →
lower the styled DOM into the same `perfect_print_layout::flow::ContentBlock`s
the native `Document` builder produces → the existing `FlowLayoutEngine` →
`DocumentModel` → the existing PDF/raster/print backends. Because there is no
second rendering path and no external process, output is deterministic and
safe to assert on in CI.

## Supported HTML/CSS subset (the contract)

**Block elements:** `h1`–`h6`, `p`, `div`, `blockquote`, `br`, `hr`, `ul`,
`ol`, `li`, `table`/`thead`/`tbody`/`tr`/`td`/`th`, `img`.

**Inline elements:** `b`/`strong`, `i`/`em`, `u`, `s`/`strike`/`del`, `span`,
`a` (rendered as styled text, no link annotation yet), `code`.

**Ignored (warning, not error):** `script`, `style` (consumed as CSS), `head`
metadata except `<title>`. Unknown tags are treated as `div` (block context)
or `span` (inline context) by display default, with a warning.

**CSS length units:** every property below that accepts a length accepts
`pt` (default/bare-number unit), `px` (96dpi → points ×0.75), `em` (relative
to the parent's resolved font size), `in` (×72), `cm` (×72/2.54), `mm`
(×72/25.4), and `pc` (×12). `%` is parsed but not resolved (treated as
unsupported, produces a warning). This applies uniformly to font sizes,
margins, padding, letter-spacing, `@page { size: ... }`, and
`left`/`top`/`width` on absolutely positioned elements — so a template
authored entirely in inches (e.g. `@page { size: 8.5in 11in }`,
`left: 0.5in`) resolves correctly without any pre-conversion.

**CSS properties:**
- `font-family`
- `font-size` (see length units above)
- `font-weight` (`normal`/`bold`/100–900, ≥700 is bold)
- `font-style` (`normal`/`italic`/`oblique`)
- `color` (`#rgb`, `#rrggbb`, `rgb(r,g,b)`, 16 named CSS colors)
- `text-align` (`left|right|center|justify`)
- `line-height` (unitless multiplier or length)
- `text-decoration` (`underline`, `line-through`)
- `margin-top`/`margin-bottom` (block spacing, collapsed to a single `Gap`
  between adjacent blocks — the larger of the previous block's
  `margin-bottom` and the next block's `margin-top` wins)
- `letter-spacing`
- `background-color` (table cells only)
- `padding` (table cells only)
- `page-break-before: always` / `page-break-after: always`
- `break-before: page` / `break-after: page`
- `position` (`absolute`, `relative`; see [`position: absolute`](#position-absolute) below — other values, e.g. `fixed`/`sticky`, are unsupported and warn)
- `left`/`top` (any supported length unit; only meaningful together with `position: absolute`)
- `width` on `position: absolute` elements (any supported length unit; sets the positioned box's layout/wrap width)

## `position: absolute`

A `div` with `position: absolute` is taken out of the normal document flow
and converted to `perfect_print_layout::flow::ContentBlock::Positioned { x,
y, width, blocks }` instead of an ordinary block:

- **`x`/`y`** come from `left`/`top` (any supported length unit), resolved
  relative to the content-area origin (inside the page margins — the same
  origin every other block's coordinates are relative to). Missing
  `left`/`top` default to `0`.
- **`width`** comes from the CSS `width` declaration; if absent it defaults
  to the remaining content width from `x` to the right content-area edge.
- The element's children convert recursively as normal blocks — paragraphs,
  tables, images, lists — laid out inside that `width`, and are rendered
  translated by `(x, y)`.
- The positioned element does **not** advance the surrounding flow's cursor:
  content before and after it lays out exactly as if the positioned element
  were absent. This matches CSS's out-of-flow semantics for
  `position: absolute`.
- **Overflow is not clipped.** Content taller than the remaining space on
  the page is rendered past the page edge rather than being paginated or
  cropped — this matches real CSS `position: absolute` (which does not
  implicitly paginate an out-of-flow element), but it does mean a
  positioned box with `overflow: hidden` and more content than its `height`
  will visibly overflow instead of being clipped; `height` and `overflow`
  are still unsupported properties and produce a warning for this reason.
- `position: relative` is accepted as a **silent no-op for flow purposes**:
  the element stays in normal flow (open/close block, margins, walk its
  children normally) and does not itself establish a new coordinate origin,
  because content-area-relative coordinates already are that origin in this
  renderer. `left`/`top` on a `position: relative` element currently have no
  effect (they are only acted on for `position: absolute`).
- Only `div` elements are checked for `position: absolute` today; other
  block-level tags with `position: absolute` styling are not special-cased.

**Selectors:** `tag`, `.class`, `#id`, `tag.class`, and comma lists.
Specificity: `id` (100) > `class` (10) > `tag` (1); a later rule wins ties.
Inline `style=""` beats all selector-based rules.

**`@page`:** `@page { size: ...; margin: ... }` — `size` accepts
`letter|a4|legal|<width> <height>`; margin lengths map onto the resolved
`PageSetup`'s margins.

**Default user-agent stylesheet** (hard-coded, applied before any document
`<style>`/inline styles):

| Selector | font-size | weight |
|---|---|---|
| `h1` | 24pt | bold |
| `h2` | 18pt | bold |
| `h3` | 14pt | bold |
| `h4`–`h6` | 12pt | bold |
| `body`/`p`/`li` | 12pt | normal |

Font family defaults to Helvetica, color to black. `blockquote` gets a 36pt
left indent. `code` defaults to Courier. Margins between blocks default to
0.5× the element's font-size (top and bottom); heading margins default to
0.75×.

## Precedence rules

- **Page setup:** an explicit `HtmlDocument::page_settings(...)` call always
  wins; otherwise a document `@page` rule wins; otherwise the letter default
  (612×792pt, 72pt margins).
- **Title:** an explicit `HtmlDocument::title(...)` call always wins;
  otherwise the HTML `<title>` element is used if present.
- **CSS cascade:** inline `style=""` > id selector > class selector > tag
  selector > later rule wins ties > the hard-coded UA stylesheet.
- **Inheritance:** font family/size/weight/style, color, text-align,
  line-height, letter-spacing, and text-decoration (underline/strikethrough)
  are inherited down the DOM the way real browsers inherit them — once set,
  a decoration stays set for descendants. Margins, background/padding
  (table cells), and page-break flags are per-element and not inherited.

## Graceful-degradation policy

`convert()` never hard-errors on unsupported markup or CSS (mirroring the
project's `Strictness::Warn` philosophy elsewhere in the workspace). Anything
outside the supported subset — an unknown tag, an unparseable CSS value, an
unsupported property, a blocked image resource — is recorded as a `String` in
`ConvertedDocument::warnings` / `HtmlRenderResult::warnings` and the rest of
the document still renders. The CLI's `render-html --strict` flag turns a
non-empty warning list into a process exit code of 1 (warnings are still
printed to stderr either way); without `--strict`, the command exits 0 and
prints warnings to stderr as advisories.

Hard errors (`HtmlRenderError`) are reserved for structural problems that
would make the pipeline correctness meaningless to continue: empty HTML
input, HTML/PDF exceeding the configured `ResourcePolicy` size limits, or the
underlying PDF/raster backend failing.

## `ResourcePolicy` security model

`HtmlDocument`s are offline by default (`ResourcePolicy::offline()`):

- **No script execution** — `<script>` tags are never executed; they are
  dropped with a warning. This pipeline has no JS engine at all.
- **No network fetches** — `http://`/`https://` resource URLs are rejected
  unless the policy explicitly enables network access; there is currently no
  network client wired into the pipeline regardless, so remote images always
  degrade to a warning + skipped image.
- **`data:` URIs are always allowed** — embedded/inlined images (e.g.
  base64-encoded PNGs) work without any policy configuration, since they
  can't read anything outside the HTML document itself.
- **Local filesystem access requires an explicit allowlisted root** —
  `file://` URLs and bare local paths are rejected unless
  `ResourcePolicy::with_local_base_directory(dir)` has been called; the
  target path is canonicalized and must resolve inside that directory (this
  blocks `../` traversal and symlink escapes). The CLI's `--base-dir` flag
  wires this up.
- **Size limits** are enforced at three points: input HTML
  (`max_html_bytes`, default 8 MiB), decoded resources such as images
  (`max_resource_bytes`, default 32 MiB), and rendered PDF output
  (`max_pdf_bytes`, default 128 MiB, enforced in
  `HtmlRenderResult::to_pdf_bytes()` via
  `ResourcePolicy::validate_pdf_bytes`). Each is configurable via
  `ResourcePolicy::with_max_*_bytes`.
- **Page dimensions are bounded** (`HtmlPageSettings::validate()`) to at most
  14,400 points per side, preventing pathological `@page` sizes.

None of these checks depend on well-formed HTML — a hostile or malformed
document degrades to warnings and skipped content rather than executing
anything or reading outside the sandboxed root.
