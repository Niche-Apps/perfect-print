#include "poster_tiles.h"

#include <stddef.h>

static double pp_min(double a, double b) { return a < b ? a : b; }
static double pp_max(double a, double b) { return a > b ? a : b; }

static int pp_finite_positive(double v) {
    return v > 0.0 && v == v;
}

/// How many tiles an axis needs. 1 when content fits (including the 0.5pt
/// epsilon); otherwise ceil, clamped to MAX_TILE_AXIS. Last cell is the
/// remainder — the grid is not forced equal.
static uint32_t pp_axis_tiles(double draw, double tile) {
    if (!pp_finite_positive(draw) || !pp_finite_positive(tile)) {
        return 1;
    }
    if (draw <= tile + PERFECT_PRINT_FIT_EPSILON) {
        return 1;
    }
    double ratio = draw / tile;
    uint32_t n = (uint32_t)ratio;
    if ((double)n < ratio) {
        n += 1;
    }
    if (n < 1) {
        n = 1;
    }
    if (n > PERFECT_PRINT_MAX_TILE_AXIS) {
        n = PERFECT_PRINT_MAX_TILE_AXIS;
    }
    return n;
}

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
) {
    if (!pp_finite_positive(paper_w)) {
        paper_w = 612.0;
    }
    if (!pp_finite_positive(paper_h)) {
        paper_h = 792.0;
    }

    if (!pp_finite_positive(imageable_w) || !pp_finite_positive(imageable_h)) {
        imageable_x = 0.0;
        imageable_y = 0.0;
        imageable_w = paper_w;
        imageable_h = paper_h;
    }

    double join = PERFECT_PRINT_JOIN_MARGIN_PT;
    double jx = join;
    double jy = join;
    double jw = paper_w - 2.0 * join;
    double jh = paper_h - 2.0 * join;
    if (jw < 1.0) {
        jx = 0.0;
        jw = paper_w;
    }
    if (jh < 1.0) {
        jy = 0.0;
        jh = paper_h;
    }

    double x0 = pp_max(imageable_x, jx);
    double y0 = pp_max(imageable_y, jy);
    double x1 = pp_min(imageable_x + imageable_w, jx + jw);
    double y1 = pp_min(imageable_y + imageable_h, jy + jh);
    double w = x1 - x0;
    double h = y1 - y0;
    if (w < 1.0 || h < 1.0) {
        x0 = imageable_x;
        y0 = imageable_y;
        w = imageable_w;
        h = imageable_h;
    }

    if (out_x) *out_x = x0;
    if (out_y) *out_y = y0;
    if (out_w) *out_w = w;
    if (out_h) *out_h = h;
}

double PerfectPrintPosterScale(
    uint8_t scaling_mode,
    double custom_scale,
    double panel_scale,
    double media_w,
    double media_h,
    double tile_w,
    double tile_h
) {
    if (!pp_finite_positive(media_w)) media_w = 1.0;
    if (!pp_finite_positive(media_h)) media_h = 1.0;
    if (!pp_finite_positive(tile_w)) tile_w = 1.0;
    if (!pp_finite_positive(tile_h)) tile_h = 1.0;

    double sx = tile_w / media_w;
    double sy = tile_h / media_h;
    double base = pp_min(sx, sy);
    switch (scaling_mode) {
        case 0: base = pp_min(sx, sy); break; /* FitToPage */
        case 1: base = pp_max(sx, sy); break; /* FillPage */
        case 2: base = 1.0; break;            /* None (1:1, then panel Scale) */
        case 3: base = custom_scale > 0.01 ? custom_scale : 0.01; break;
        default: break;
    }

    if (!pp_finite_positive(panel_scale)) {
        panel_scale = 1.0;
    }
    return base * panel_scale;
}

PerfectPrintPosterGrid PerfectPrintPosterGridFromScaled(
    double scaled_w,
    double scaled_h,
    double tile_w,
    double tile_h
) {
    PerfectPrintPosterGrid grid;
    grid.cols = pp_axis_tiles(scaled_w, tile_w);
    grid.rows = pp_axis_tiles(scaled_h, tile_h);
    grid.pages = grid.cols * grid.rows;
    if (grid.pages < 1) {
        grid.pages = 1;
    }
    return grid;
}

PerfectPrintPosterGrid PerfectPrintComputePosterGrid(
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h
) {
    if (!pp_finite_positive(scale)) {
        scale = 1.0;
    }
    return PerfectPrintPosterGridFromScaled(media_w * scale, media_h * scale, tile_w, tile_h);
}

int32_t PerfectPrintPosterTileAt(
    uint32_t index,
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h,
    PerfectPrintPosterTile *out
) {
    if (!out) {
        return 0;
    }
    if (!pp_finite_positive(scale)) {
        scale = 1.0;
    }
    if (!pp_finite_positive(media_w)) media_w = 1.0;
    if (!pp_finite_positive(media_h)) media_h = 1.0;
    if (!pp_finite_positive(tile_w)) tile_w = 1.0;
    if (!pp_finite_positive(tile_h)) tile_h = 1.0;

    double scaled_w = media_w * scale;
    double scaled_h = media_h * scale;
    PerfectPrintPosterGrid grid = PerfectPrintPosterGridFromScaled(scaled_w, scaled_h, tile_w, tile_h);
    if (index >= grid.pages) {
        return 0;
    }

    uint32_t col = index % grid.cols;
    uint32_t row = index / grid.cols;

    double start_x = (double)col * tile_w;
    double start_y = (double)row * tile_h;
    double dest_w = pp_max(pp_min(scaled_w - start_x, tile_w), 1.0);
    double dest_h = pp_max(pp_min(scaled_h - start_y, tile_h), 1.0);

    /* Media-space crop. Top-down: row 0 is the top of the chart. No overlap
       — the next tile starts where this one ends. */
    double src_x = start_x / scale;
    double src_y = start_y / scale;
    double src_w = dest_w / scale;
    double src_h = dest_h / scale;
    if (src_x + src_w > media_w) {
        src_w = media_w - src_x;
    }
    if (src_y + src_h > media_h) {
        src_h = media_h - src_y;
    }
    if (src_w < 0.0) src_w = 0.0;
    if (src_h < 0.0) src_h = 0.0;

    out->col = col;
    out->row = row;
    out->index = index;
    out->src_x = src_x;
    out->src_y = src_y;
    out->src_w = src_w;
    out->src_h = src_h;
    out->dest_w = dest_w;
    out->dest_h = dest_h;
    return 1;
}

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
) {
    double tile_x = 0.0, tile_y = 0.0, tile_w = 0.0, tile_h = 0.0;
    PerfectPrintPosterContentBounds(
        paper_w, paper_h,
        imageable_x, imageable_y, imageable_w, imageable_h,
        &tile_x, &tile_y, &tile_w, &tile_h);
    (void)tile_x;
    (void)tile_y;
    double scale = PerfectPrintPosterScale(
        scaling_mode, custom_scale, panel_scale,
        media_w, media_h, tile_w, tile_h);
    PerfectPrintPosterGrid grid = PerfectPrintComputePosterGrid(
        media_w, media_h, scale, tile_w, tile_h);
    if (out_cols) *out_cols = grid.cols;
    if (out_rows) *out_rows = grid.rows;
    if (out_pages) *out_pages = grid.pages;
    if (out_scale) *out_scale = scale;
    return 1;
}

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
) {
    PerfectPrintPosterTile tile;
    if (!PerfectPrintPosterTileAt(index, media_w, media_h, scale, tile_w, tile_h, &tile)) {
        return 0;
    }
    if (out_col) *out_col = tile.col;
    if (out_row) *out_row = tile.row;
    if (out_index) *out_index = tile.index;
    if (out_src_x) *out_src_x = tile.src_x;
    if (out_src_y) *out_src_y = tile.src_y;
    if (out_src_w) *out_src_w = tile.src_w;
    if (out_src_h) *out_src_h = tile.src_h;
    if (out_dest_w) *out_dest_w = tile.dest_w;
    if (out_dest_h) *out_dest_h = tile.dest_h;
    return 1;
}
