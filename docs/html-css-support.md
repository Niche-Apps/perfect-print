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
(×72/25.4), and `pc` (×12). `%` is resolved only for `<img>` `width`/`height`
(against the nearest `position: absolute` ancestor's box — see [image
sizing](#image-sizing)); everywhere else `%` is parsed but not resolved
(treated as unsupported, produces a warning). Non-percentage units apply
uniformly to font sizes, margins, padding, letter-spacing,
`@page { size: ... }`, and `left`/`top`/`width`/`height` on absolutely
positioned elements — so a template authored entirely in inches (e.g.
`@page { size: 8.5in 11in }`,
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
- `height` on `position: absolute` elements (any supported length unit; establishes the positioned box's height for percentage resolution — see [image sizing](#image-sizing) below. On other elements `height` is parsed but has no layout effect since block heights are content-driven.)
- `width`/`height` as a percentage (`%`) on an `<img>` — resolved against the nearest enclosing `position: absolute` container's box (see [image sizing](#image-sizing))
- `object-fit` (`contain`, `fill`) on `<img>` (see [image sizing](#image-sizing))
- `white-space` (`normal`, `pre-wrap`, `pre-line`; inherited — see [`white-space`](#white-space) below. Other values, e.g. `nowrap`/`pre`/`break-spaces`, are unsupported and warn.)

## `white-space`

`white-space: normal` (the default) matches HTML's usual behavior: any run
of spaces/tabs/newlines in source text collapses to a single space, so a
literal `\n` in the DOM (common in server-rendered templates that interpolate
multi-line values into a `<div>` without `<br>`) collapses away and the text
renders as one run-on line.

`white-space: pre-wrap` and `white-space: pre-line` both preserve literal
`\n` characters as forced line breaks — the same mechanism `<br>` uses
internally (splitting the span stream on a marker, then laying each segment
out as its own line/paragraph). Runs of interior spaces/tabs are still
collapsed to a single space in both modes.

**Simplification:** per the CSS spec, `pre-wrap` should additionally preserve
runs of interior whitespace (only wrapping, not collapsing, spaces). This
converter does not make that distinction — `pre-wrap` is treated identically
to `pre-line` (newlines preserved, interior whitespace runs collapsed to one
space). This covers the common case (server-rendered address/note blocks with
real newlines) without the added complexity of a true whitespace-preserving
text run type in the layout engine. Revisit if a template needs
whitespace-significant `pre-wrap` rendering.

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
- **`height`** (any supported length unit) establishes the positioned box's
  height, used only to resolve percentage `width`/`height` on descendant
  `<img>` elements (see [image sizing](#image-sizing)) — it does not clip
  or paginate the box's own content.
- **Overflow is not clipped.** Content taller than the remaining space on
  the page is rendered past the page edge rather than being paginated or
  cropped — this matches real CSS `position: absolute` (which does not
  implicitly paginate an out-of-flow element), but it does mean a
  positioned box with `overflow: hidden` and more content than its `height`
  will visibly overflow instead of being clipped; `overflow` itself is
  still an unsupported property and produces a warning.
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

## Image sizing

`<img>` destination size in points is resolved in this precedence order —
a print page must never render an image beyond its declared container, or,
absent a container, beyond the page's content width:

1. **CSS `width`/`height`.** Any supported absolute length, or `%`
   (resolved against the nearest enclosing `position: absolute`
   container's box — its CSS `width`/`height`; a `%` with no enclosing
   positioned container, or against a container with no resolvable CSS
   `height`, has nothing to resolve against and falls through to the next
   rule). If only one of `width`/`height` resolves, the other is derived
   preserving the image's natural aspect ratio. If both resolve:
   - `object-fit: contain` scales the natural image to fit inside the
     resolved `width`×`height` box, preserving aspect ratio (never
     cropping, never distorting).
   - Otherwise (the CSS default, `fill`, or `object-fit` unspecified) the
     image is stretched to exactly `width`×`height`, matching real CSS
     `object-fit: fill` semantics.
2. **Legacy HTML `width`/`height` attributes** (not CSS), honored together
   for backward compatibility, if CSS resolved neither dimension.
3. **No resolvable CSS size, inside a positioned container with a known
   box:** fit to the container's box (contain semantics, never upscaling
   past natural size) rather than rendering at natural pixel size — this is
   what prevents an oversized logo dropped into a template's image slot
   from covering the rest of the page.
4. **No resolvable CSS size, no positioned container:** capped at the
   remaining content width (from the current indent to the content-area's
   right edge), preserving aspect ratio, never upscaled past natural size.

This is what makes `<div style="position:absolute;...;width:Wpt;height:Hpt;overflow:hidden"><img style="width:100%;height:100%;object-fit:contain" .../></div>`
— the pattern PlainBooks' invoice templates emit — size correctly instead
of falling through to the image's natural pixel dimensions (previously: a
multi-thousand-pixel logo would render far larger than its template box,
opaque, covering all earlier-drawn page content).

`overflow` itself remains unsupported (see [`position:
absolute`](#position-absolute) above) — sizing an image to fit its box
makes `overflow: hidden` moot for the common "image fills its box" case,
but a box with `overflow: hidden` and *other* content taller than its
`height` will still visibly overflow rather than being clipped.

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
