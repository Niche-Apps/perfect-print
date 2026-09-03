#ifndef PERFECT_PRINT_POSTER_TILES_H
#define PERFECT_PRINT_POSTER_TILES_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Tape/join strip between poster tiles, matching Families' page margin.
/// Tiles do not overlap through content; this band is for taping and chrome.
#define PERFECT_PRINT_JOIN_MARGIN_PT 28.0

/// Registration tick length in the join margin (must stay ≤ 8pt).
#define PERFECT_PRINT_TICK_LENGTH_PT 6.0
#define PERFECT_PRINT_TICK_MAX_PT 8.0

/// Content that is only this much larger than a tile still counts as a fit.
#define PERFECT_PRINT_FIT_EPSILON 0.5

/// Cap on tiles per axis so a huge Scale cannot create an unbounded job.
#define PERFECT_PRINT_MAX_TILE_AXIS 16

/// Upper bound on tiles in one source page (axis cap squared).
#define PERFECT_PRINT_MAX_TILES (PERFECT_PRINT_MAX_TILE_AXIS * PERFECT_PRINT_MAX_TILE_AXIS)

/// Families print page fill (#fff / PRINT_PAGE_FILL) and ink (#111).
#define PERFECT_PRINT_PAGE_FILL_R 255
#define PERFECT_PRINT_PAGE_FILL_G 255
#define PERFECT_PRINT_PAGE_FILL_B 255
#define PERFECT_PRINT_PAGE_INK_R 0x11
#define PERFECT_PRINT_PAGE_INK_G 0x11
#define PERFECT_PRINT_PAGE_INK_B 0x11

/// Per-channel distance from #fff still counted as background (anti-alias).
#define PERFECT_PRINT_PAGE_FILL_SLACK 16

/// Keep a tile if it has at least this many non-background samples.
/// Low so a name on an otherwise empty edge tile is not dropped.
#define PERFECT_PRINT_TILE_INK_MIN_SAMPLES 2

/// Long-side cap (px) for the occupancy bitmap used by the print view.
#define PERFECT_PRINT_INK_SAMPLE_MAX 192

/// Margin-only "n of N" label size (pt). Hidden when N == 1.
#define PERFECT_PRINT_SHEET_LABEL_SIZE 7.0

typedef struct {
    uint32_t cols;
    uint32_t rows;
    uint32_t pages;
} PerfectPrintPosterGrid;

typedef struct {
    uint32_t col;
    uint32_t row;
    uint32_t index; /* 0-based, row-major: L→R, then T→B */
    double src_x;   /* media-relative, top-down (row 0 is the top of the chart) */
    double src_y;
    double src_w;
    double src_h;
    double dest_w;  /* remainder tiles are smaller; not stretched */
    double dest_h;
} PerfectPrintPosterTile;

/// One tile that survived blank-page suppression.
typedef struct {
    uint32_t col;
    uint32_t row;
    uint32_t grid_index;  /* original row-major index in the full rectangle */
    uint32_t print_index; /* 0-based among kept tiles (post-filter n-1) */
    int32_t join_right;   /* 1 if (col+1, row) is also kept */
    int32_t join_bottom;  /* 1 if (col, row+1) is also kept */
    PerfectPrintPosterTile tile;
} PerfectPrintPosterKeptTile;

/// Geometric grid plus the post-filter kept list (always at least 1 tile).
typedef struct {
    uint32_t cols;  /* full rectangular grid */
    uint32_t rows;
    uint32_t pages; /* cols * rows */
    uint32_t kept;  /* post-filter page count, ≥ 1 */
    PerfectPrintPosterKeptTile tiles[PERFECT_PRINT_MAX_TILES];
} PerfectPrintPosterKeptGrid;

/// Content (tile destination) rect: paper inset by the 28pt join, intersected
/// with the printer's imageable bounds. Falls back to imageable (or paper)
/// when the intersection would be empty.
void PerfectPrintPosterContentBounds(
    double paper_w,
    double paper_h,
    double imageable_x,
    double imageable_y,
    double imageable_w,
    double imageable_h,
    double *out_x,
    double *out_y,
    double *out_w,
    double *out_h
);

/// Combined scale: scaling mode (0 FitToPage, 1 FillPage, 2 None, 3 Custom)
/// times the native panel's scalingFactor. Fit/Fill ratios use the tile
/// destination (content bounds), not the full paper.
double PerfectPrintPosterScale(
    uint8_t scaling_mode,
    double custom_scale,
    double panel_scale,
    double media_w,
    double media_h,
    double tile_w,
    double tile_h
);

/// Tile grid for already-scaled content versus one tile's destination size.
PerfectPrintPosterGrid PerfectPrintPosterGridFromScaled(
    double scaled_w,
    double scaled_h,
    double tile_w,
    double tile_h
);

/// Tile grid for a source page: ceil(media × scale / tile), 1 when it fits.
PerfectPrintPosterGrid PerfectPrintComputePosterGrid(
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h
);

/// Contiguous, non-overlapping crop for a 0-based row-major tile index.
/// Returns 1 on success, 0 if `index` is out of range.
int32_t PerfectPrintPosterTileAt(
    uint32_t index,
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h,
    PerfectPrintPosterTile *out
);

/// Hermetic inspect: content bounds + scale + grid for one source page.
/// Does not need AppKit. Used by Rust tests on every platform.
int32_t perfect_print_native_inspect_poster_grid(
    double media_w,
    double media_h,
    double paper_w,
    double paper_h,
    double imageable_x,
    double imageable_y,
    double imageable_w,
    double imageable_h,
    uint8_t scaling_mode,
    double custom_scale,
    double panel_scale,
    uint32_t *out_cols,
    uint32_t *out_rows,
    uint32_t *out_pages,
    double *out_scale
);

/// Hermetic inspect for one tile's crop (0-based index).
int32_t perfect_print_native_inspect_poster_tile(
    uint32_t index,
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h,
    uint32_t *out_col,
    uint32_t *out_row,
    uint32_t *out_index,
    double *out_src_x,
    double *out_src_y,
    double *out_src_w,
    double *out_src_h,
    double *out_dest_w,
    double *out_dest_h
);

/// 1 if RGB is far enough from #fff to count as chart ink.
int32_t PerfectPrintPixelIsInk(uint8_t r, uint8_t g, uint8_t b);

/// Count non-background pixels in a media-space rect mapped onto a bitmap.
/// `pixels` is 8-bit RGB (channels=3) or RGBA (channels=4), top-down,
/// row 0 = media top (src_y = 0). `stride` is bytes per row.
uint32_t PerfectPrintCountInkPixels(
    const uint8_t *pixels,
    uint32_t pix_w,
    uint32_t pix_h,
    uint32_t stride,
    uint32_t channels,
    double media_w,
    double media_h,
    double src_x,
    double src_y,
    double src_w,
    double src_h
);

/// 1 if the sampled region has enough ink to keep the tile.
int32_t PerfectPrintPosterRegionHasInk(
    const uint8_t *pixels,
    uint32_t pix_w,
    uint32_t pix_h,
    uint32_t stride,
    uint32_t channels,
    double media_w,
    double media_h,
    double src_x,
    double src_y,
    double src_w,
    double src_h
);

/// Drop tiles whose PDF/image region is blank (or below the ink threshold).
/// `pixels == NULL` keeps the full geometric grid (safe fallback).
/// If every tile is empty, tile 0 is kept so the job is never zero pages.
void PerfectPrintFilterPosterTiles(
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h,
    const uint8_t *pixels,
    uint32_t pix_w,
    uint32_t pix_h,
    uint32_t stride,
    uint32_t channels,
    PerfectPrintPosterKeptGrid *out
);

/// Same filter using an axis-aligned ink rect in media space (top-down).
/// A tile is kept when its crop intersects the rect. Empty/invalid ink
/// keeps tile 0.
void PerfectPrintFilterPosterTilesFromInkRect(
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h,
    double ink_x,
    double ink_y,
    double ink_w,
    double ink_h,
    PerfectPrintPosterKeptGrid *out
);

/// Hermetic inspect: geometric grid + post-filter page count from an ink rect.
int32_t perfect_print_native_inspect_poster_kept_from_ink_rect(
    double media_w,
    double media_h,
    double paper_w,
    double paper_h,
    double imageable_x,
    double imageable_y,
    double imageable_w,
    double imageable_h,
    uint8_t scaling_mode,
    double custom_scale,
    double panel_scale,
    double ink_x,
    double ink_y,
    double ink_w,
    double ink_h,
    uint32_t *out_cols,
    uint32_t *out_rows,
    uint32_t *out_pages,
    uint32_t *out_kept,
    double *out_scale
);

/// Hermetic inspect for one kept tile (0-based print index after filtering).
int32_t perfect_print_native_inspect_poster_kept_tile(
    uint32_t kept_index,
    double media_w,
    double media_h,
    double paper_w,
    double paper_h,
    double imageable_x,
    double imageable_y,
    double imageable_w,
    double imageable_h,
    uint8_t scaling_mode,
    double custom_scale,
    double panel_scale,
    double ink_x,
    double ink_y,
    double ink_w,
    double ink_h,
    uint32_t *out_col,
    uint32_t *out_row,
    uint32_t *out_print_index,
    uint32_t *out_grid_index,
    int32_t *out_join_right,
    int32_t *out_join_bottom,
    double *out_src_x,
    double *out_src_y,
    double *out_src_w,
    double *out_src_h
);

/// Hermetic inspect: post-filter page count from a packed occupancy bitmap.
int32_t perfect_print_native_inspect_poster_kept_from_bitmap(
    double media_w,
    double media_h,
    double paper_w,
    double paper_h,
    double imageable_x,
    double imageable_y,
    double imageable_w,
    double imageable_h,
    uint8_t scaling_mode,
    double custom_scale,
    double panel_scale,
    const uint8_t *pixels,
    uint32_t pix_w,
    uint32_t pix_h,
    uint32_t stride,
    uint32_t channels,
    uint32_t *out_cols,
    uint32_t *out_rows,
    uint32_t *out_pages,
    uint32_t *out_kept,
    double *out_scale
);

#ifdef __cplusplus
}
#endif

#endif /* PERFECT_PRINT_POSTER_TILES_H */
