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

#ifdef __cplusplus
}
#endif

#endif /* PERFECT_PRINT_POSTER_TILES_H */
