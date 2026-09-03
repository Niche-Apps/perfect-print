#include "poster_tiles.h"

#include <stddef.h>
#include <string.h>

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

int32_t PerfectPrintPixelIsInk(uint8_t r, uint8_t g, uint8_t b) {
    const int slack = PERFECT_PRINT_PAGE_FILL_SLACK;
    const int floor_v = 255 - slack;
    return r < floor_v || g < floor_v || b < floor_v;
}

static int pp_floor_nonneg(double v) {
    if (v <= 0.0) {
        return 0;
    }
    return (int)v;
}

static int pp_ceil_nonneg(double v) {
    if (v <= 0.0) {
        return 0;
    }
    int i = (int)v;
    if ((double)i < v) {
        i += 1;
    }
    return i;
}

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
) {
    if (!pixels || pix_w == 0 || pix_h == 0) {
        return 0;
    }
    if (channels != 3 && channels != 4) {
        return 0;
    }
    if (stride < pix_w * channels) {
        return 0;
    }
    if (!pp_finite_positive(media_w) || !pp_finite_positive(media_h)) {
        return 0;
    }
    if (!(src_w > 0.0) || !(src_h > 0.0)) {
        return 0;
    }

    int x0 = pp_floor_nonneg(src_x / media_w * (double)pix_w);
    int x1 = pp_ceil_nonneg((src_x + src_w) / media_w * (double)pix_w);
    int y0 = pp_floor_nonneg(src_y / media_h * (double)pix_h);
    int y1 = pp_ceil_nonneg((src_y + src_h) / media_h * (double)pix_h);
    if (x0 < 0) x0 = 0;
    if (y0 < 0) y0 = 0;
    if (x1 > (int)pix_w) x1 = (int)pix_w;
    if (y1 > (int)pix_h) y1 = (int)pix_h;
    if (x1 <= x0 || y1 <= y0) {
        return 0;
    }

    uint32_t ink = 0;
    for (int y = y0; y < y1; y++) {
        const uint8_t *row = pixels + (size_t)y * (size_t)stride;
        for (int x = x0; x < x1; x++) {
            const uint8_t *p = row + (size_t)x * (size_t)channels;
            if (PerfectPrintPixelIsInk(p[0], p[1], p[2])) {
                ink += 1;
            }
        }
    }
    return ink;
}

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
) {
    uint32_t ink = PerfectPrintCountInkPixels(
        pixels, pix_w, pix_h, stride, channels,
        media_w, media_h, src_x, src_y, src_w, src_h);
    if (ink == 0) {
        return 0;
    }

    int x0 = pp_floor_nonneg(src_x / media_w * (double)pix_w);
    int x1 = pp_ceil_nonneg((src_x + src_w) / media_w * (double)pix_w);
    int y0 = pp_floor_nonneg(src_y / media_h * (double)pix_h);
    int y1 = pp_ceil_nonneg((src_y + src_h) / media_h * (double)pix_h);
    if (x0 < 0) x0 = 0;
    if (y0 < 0) y0 = 0;
    if (x1 > (int)pix_w) x1 = (int)pix_w;
    if (y1 > (int)pix_h) y1 = (int)pix_h;
    int samples = (x1 > x0 && y1 > y0) ? (x1 - x0) * (y1 - y0) : 0;
    uint32_t need = PERFECT_PRINT_TILE_INK_MIN_SAMPLES;
    if (samples > 0 && need > (uint32_t)samples) {
        need = 1;
    }
    return ink >= need;
}

static int pp_rects_intersect(
    double ax, double ay, double aw, double ah,
    double bx, double by, double bw, double bh
) {
    if (!(aw > 0.0) || !(ah > 0.0) || !(bw > 0.0) || !(bh > 0.0)) {
        return 0;
    }
    return ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah;
}

static void pp_build_kept(
    double media_w,
    double media_h,
    double scale,
    double tile_w,
    double tile_h,
    uint8_t *keep,
    PerfectPrintPosterKeptGrid *out
) {
    memset(out, 0, sizeof(*out));
    if (!pp_finite_positive(scale)) {
        scale = 1.0;
    }
    if (!pp_finite_positive(media_w)) media_w = 1.0;
    if (!pp_finite_positive(media_h)) media_h = 1.0;
    if (!pp_finite_positive(tile_w)) tile_w = 1.0;
    if (!pp_finite_positive(tile_h)) tile_h = 1.0;

    PerfectPrintPosterGrid grid = PerfectPrintPosterGridFromScaled(
        media_w * scale, media_h * scale, tile_w, tile_h);
    out->cols = grid.cols;
    out->rows = grid.rows;
    out->pages = grid.pages;

    uint32_t nkeep = 0;
    for (uint32_t i = 0; i < grid.pages; i++) {
        if (keep[i]) {
            nkeep += 1;
        }
    }
    if (nkeep == 0 && grid.pages > 0) {
        keep[0] = 1;
    }

    uint32_t out_i = 0;
    for (uint32_t i = 0; i < grid.pages && out_i < PERFECT_PRINT_MAX_TILES; i++) {
        if (!keep[i]) {
            continue;
        }
        PerfectPrintPosterTile tile;
        if (!PerfectPrintPosterTileAt(i, media_w, media_h, scale, tile_w, tile_h, &tile)) {
            continue;
        }
        out->tiles[out_i].col = tile.col;
        out->tiles[out_i].row = tile.row;
        out->tiles[out_i].grid_index = i;
        out->tiles[out_i].print_index = out_i;
        out->tiles[out_i].join_right = 0;
        out->tiles[out_i].join_bottom = 0;
        out->tiles[out_i].tile = tile;
        out_i += 1;
    }
    out->kept = out_i > 0 ? out_i : 1;

    uint8_t occupied[PERFECT_PRINT_MAX_TILE_AXIS][PERFECT_PRINT_MAX_TILE_AXIS];
    memset(occupied, 0, sizeof(occupied));
    for (uint32_t i = 0; i < out->kept; i++) {
        uint32_t c = out->tiles[i].col;
        uint32_t r = out->tiles[i].row;
        if (c < PERFECT_PRINT_MAX_TILE_AXIS && r < PERFECT_PRINT_MAX_TILE_AXIS) {
            occupied[r][c] = 1;
        }
    }
    for (uint32_t i = 0; i < out->kept; i++) {
        uint32_t c = out->tiles[i].col;
        uint32_t r = out->tiles[i].row;
        out->tiles[i].join_right =
            (c + 1 < grid.cols && c + 1 < PERFECT_PRINT_MAX_TILE_AXIS && occupied[r][c + 1])
                ? 1
                : 0;
        out->tiles[i].join_bottom =
            (r + 1 < grid.rows && r + 1 < PERFECT_PRINT_MAX_TILE_AXIS && occupied[r + 1][c])
                ? 1
                : 0;
    }
}

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
) {
    if (!out) {
        return;
    }
    if (!pp_finite_positive(scale)) {
        scale = 1.0;
    }
    PerfectPrintPosterGrid grid = PerfectPrintComputePosterGrid(
        media_w, media_h, scale, tile_w, tile_h);
    uint8_t keep[PERFECT_PRINT_MAX_TILES];
    memset(keep, 0, sizeof(keep));

    if (!pixels) {
        for (uint32_t i = 0; i < grid.pages && i < PERFECT_PRINT_MAX_TILES; i++) {
            keep[i] = 1;
        }
        pp_build_kept(media_w, media_h, scale, tile_w, tile_h, keep, out);
        return;
    }

    for (uint32_t i = 0; i < grid.pages && i < PERFECT_PRINT_MAX_TILES; i++) {
        PerfectPrintPosterTile tile;
        if (!PerfectPrintPosterTileAt(i, media_w, media_h, scale, tile_w, tile_h, &tile)) {
            continue;
        }
        keep[i] = PerfectPrintPosterRegionHasInk(
                      pixels, pix_w, pix_h, stride, channels,
                      media_w, media_h,
                      tile.src_x, tile.src_y, tile.src_w, tile.src_h)
                      ? 1
                      : 0;
    }
    pp_build_kept(media_w, media_h, scale, tile_w, tile_h, keep, out);
}

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
) {
    if (!out) {
        return;
    }
    if (!pp_finite_positive(scale)) {
        scale = 1.0;
    }
    PerfectPrintPosterGrid grid = PerfectPrintComputePosterGrid(
        media_w, media_h, scale, tile_w, tile_h);
    uint8_t keep[PERFECT_PRINT_MAX_TILES];
    memset(keep, 0, sizeof(keep));
    for (uint32_t i = 0; i < grid.pages && i < PERFECT_PRINT_MAX_TILES; i++) {
        PerfectPrintPosterTile tile;
        if (!PerfectPrintPosterTileAt(i, media_w, media_h, scale, tile_w, tile_h, &tile)) {
            continue;
        }
        keep[i] = pp_rects_intersect(
                      tile.src_x, tile.src_y, tile.src_w, tile.src_h,
                      ink_x, ink_y, ink_w, ink_h)
                      ? 1
                      : 0;
    }
    pp_build_kept(media_w, media_h, scale, tile_w, tile_h, keep, out);
}

static void pp_inspect_scale_and_tile(
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
    double *out_scale,
    double *out_tile_w,
    double *out_tile_h
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
    if (out_scale) *out_scale = scale;
    if (out_tile_w) *out_tile_w = tile_w;
    if (out_tile_h) *out_tile_h = tile_h;
}

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
) {
    double scale = 1.0, tile_w = 1.0, tile_h = 1.0;
    pp_inspect_scale_and_tile(
        media_w, media_h, paper_w, paper_h,
        imageable_x, imageable_y, imageable_w, imageable_h,
        scaling_mode, custom_scale, panel_scale,
        &scale, &tile_w, &tile_h);
    PerfectPrintPosterKeptGrid kept;
    PerfectPrintFilterPosterTilesFromInkRect(
        media_w, media_h, scale, tile_w, tile_h,
        ink_x, ink_y, ink_w, ink_h, &kept);
    if (out_cols) *out_cols = kept.cols;
    if (out_rows) *out_rows = kept.rows;
    if (out_pages) *out_pages = kept.pages;
    if (out_kept) *out_kept = kept.kept;
    if (out_scale) *out_scale = scale;
    return 1;
}

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
) {
    double scale = 1.0, tile_w = 1.0, tile_h = 1.0;
    pp_inspect_scale_and_tile(
        media_w, media_h, paper_w, paper_h,
        imageable_x, imageable_y, imageable_w, imageable_h,
        scaling_mode, custom_scale, panel_scale,
        &scale, &tile_w, &tile_h);
    PerfectPrintPosterKeptGrid kept;
    PerfectPrintFilterPosterTilesFromInkRect(
        media_w, media_h, scale, tile_w, tile_h,
        ink_x, ink_y, ink_w, ink_h, &kept);
    if (kept_index >= kept.kept) {
        return 0;
    }
    const PerfectPrintPosterKeptTile *t = &kept.tiles[kept_index];
    if (out_col) *out_col = t->col;
    if (out_row) *out_row = t->row;
    if (out_print_index) *out_print_index = t->print_index;
    if (out_grid_index) *out_grid_index = t->grid_index;
    if (out_join_right) *out_join_right = t->join_right;
    if (out_join_bottom) *out_join_bottom = t->join_bottom;
    if (out_src_x) *out_src_x = t->tile.src_x;
    if (out_src_y) *out_src_y = t->tile.src_y;
    if (out_src_w) *out_src_w = t->tile.src_w;
    if (out_src_h) *out_src_h = t->tile.src_h;
    return 1;
}

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
) {
    double scale = 1.0, tile_w = 1.0, tile_h = 1.0;
    pp_inspect_scale_and_tile(
        media_w, media_h, paper_w, paper_h,
        imageable_x, imageable_y, imageable_w, imageable_h,
        scaling_mode, custom_scale, panel_scale,
        &scale, &tile_w, &tile_h);
    PerfectPrintPosterKeptGrid kept;
    PerfectPrintFilterPosterTiles(
        media_w, media_h, scale, tile_w, tile_h,
        pixels, pix_w, pix_h, stride, channels, &kept);
    if (out_cols) *out_cols = kept.cols;
    if (out_rows) *out_rows = kept.rows;
    if (out_pages) *out_pages = kept.pages;
    if (out_kept) *out_kept = kept.kept;
    if (out_scale) *out_scale = scale;
    return 1;
}
