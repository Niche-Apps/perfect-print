#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <Quartz/Quartz.h>
#import <ApplicationServices/ApplicationServices.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <dispatch/dispatch.h>
#include "poster_tiles.h"

typedef struct {
    uint32_t copies;
    bool landscape;
    uint8_t duplex;
    uint8_t color_mode;
    uint8_t scaling;
    double custom_scale;
    double paper_width;
    double paper_height;
    bool collate;
    uint8_t page_range_kind;
    uint32_t first_page;
    uint32_t last_page;
} PerfectPrintNativeSettings;

@interface PerfectPrintPDFView : NSView
@property(nonatomic, strong) PDFDocument *document;
@property(nonatomic) uint8_t scalingMode;
@property(nonatomic) double customScale;
@property(nonatomic, strong) NSArray<NSNumber *> *pageNumbers;
@property(nonatomic) NSSize fallbackPaperSize;
@end

@implementation PerfectPrintPDFView

- (NSPrintInfo *)currentPrintInfo {
    NSPrintOperation *operation = NSPrintOperation.currentOperation;
    return operation ? operation.printInfo : nil;
}

- (NSSize)currentPaperSize {
    NSPrintInfo *info = [self currentPrintInfo];
    if (info && info.paperSize.width > 0.0 && info.paperSize.height > 0.0) {
        return info.paperSize;
    }
    if (self.fallbackPaperSize.width > 0.0 && self.fallbackPaperSize.height > 0.0) {
        return self.fallbackPaperSize;
    }
    return NSMakeSize(612.0, 792.0);
}

- (NSRect)currentImageableBounds {
    NSPrintInfo *info = [self currentPrintInfo];
    if (info) {
        NSRect imageable = info.imageablePageBounds;
        if (NSWidth(imageable) > 1.0 && NSHeight(imageable) > 1.0) {
            return imageable;
        }
    }
    NSSize paper = [self currentPaperSize];
    return NSMakeRect(0, 0, paper.width, paper.height);
}

/// Tile destination (content) rect in paper coordinates: 28pt join ∩ imageable.
- (NSRect)contentBounds {
    NSSize paper = [self currentPaperSize];
    NSRect imageable = [self currentImageableBounds];
    double x = 0, y = 0, w = 0, h = 0;
    PerfectPrintPosterContentBounds(
        paper.width, paper.height,
        NSMinX(imageable), NSMinY(imageable), NSWidth(imageable), NSHeight(imageable),
        &x, &y, &w, &h);
    return NSMakeRect(x, y, w, h);
}

/// Panel Scale, or 1.0 when there is no current operation / the field is unset.
- (CGFloat)panelScale {
    NSPrintInfo *info = [self currentPrintInfo];
    if (!info) {
        return 1.0;
    }
    CGFloat panelScale = info.scalingFactor;
    if (panelScale <= 0.0 || !isfinite(panelScale)) {
        panelScale = 1.0;
    }
    return panelScale;
}

/// The scale factor this view applies to `media` under the current scaling
/// mode × the native panel's Scale field. Shared by pagination and drawing
/// so the crop AppKit is told to place and the crop we draw cannot drift.
///
/// FitToPage / FillPage ratios use the 28pt-inset content rect (not the
/// full paper). If there is no current operation, panel Scale is 1.0 and
/// paper falls back to `fallbackPaperSize`.
- (CGFloat)scaleForMedia:(NSRect)media {
    NSRect content = [self contentBounds];
    return (CGFloat)PerfectPrintPosterScale(
        self.scalingMode,
        self.customScale,
        [self panelScale],
        NSWidth(media),
        NSHeight(media),
        NSWidth(content),
        NSHeight(content));
}

- (PerfectPrintPosterGrid)gridForMedia:(NSRect)media {
    NSRect content = [self contentBounds];
    CGFloat scale = [self scaleForMedia:media];
    return PerfectPrintComputePosterGrid(
        NSWidth(media), NSHeight(media), scale, NSWidth(content), NSHeight(content));
}

- (NSRect)mediaForDocumentPage:(NSUInteger)documentPage1 {
    if (!self.document || documentPage1 < 1 || documentPage1 > self.document.pageCount) {
        return self.bounds;
    }
    PDFPage *page = [self.document pageAtIndex:documentPage1 - 1];
    return [page boundsForBox:kPDFDisplayBoxMediaBox];
}

/// Live page count: each selected source page becomes 1 tile when it fits
/// at the current Scale, or a row-major poster grid when it overflows.
/// Recomputed every call so the panel Scale field changes N.
- (NSUInteger)currentPrintPageCount {
    if (!self.document || self.document.pageCount == 0 || self.pageNumbers.count == 0) {
        return 0;
    }
    NSUInteger total = 0;
    for (NSNumber *number in self.pageNumbers) {
        NSRect media = [self mediaForDocumentPage:number.unsignedIntegerValue];
        PerfectPrintPosterGrid grid = [self gridForMedia:media];
        total += grid.pages;
    }
    return total > 0 ? total : 1;
}

- (BOOL)knowsPageRange:(NSRangePointer)range {
    NSUInteger pages = [self currentPrintPageCount];
    if (pages == 0) {
        return NO;
    }
    range->location = 1;
    range->length = pages;
    return YES;
}

/// Map a 1-based print-operation page onto a source PDF page and poster tile.
- (BOOL)resolvePrintPage:(NSInteger)pageNumber
           documentPage:(NSUInteger *)outDocumentPage1
              tileIndex:(uint32_t *)outTileIndex
                   grid:(PerfectPrintPosterGrid *)outGrid
                  media:(NSRect *)outMedia {
    if (pageNumber < 1) {
        return NO;
    }
    NSUInteger remaining = (NSUInteger)pageNumber;
    for (NSNumber *number in self.pageNumbers) {
        NSUInteger documentPage1 = number.unsignedIntegerValue;
        NSRect media = [self mediaForDocumentPage:documentPage1];
        PerfectPrintPosterGrid grid = [self gridForMedia:media];
        if (remaining <= grid.pages) {
            if (outDocumentPage1) *outDocumentPage1 = documentPage1;
            if (outTileIndex) *outTileIndex = (uint32_t)(remaining - 1);
            if (outGrid) *outGrid = grid;
            if (outMedia) *outMedia = media;
            return YES;
        }
        remaining -= grid.pages;
    }
    return NO;
}

- (NSRect)rectForPage:(NSInteger)pageNumber {
    (void)pageNumber;
    // Always one full imageable rect so remainder tiles stay top-left
    // aligned for taping (AppKit would center a smaller rect). Chrome
    // (ticks, "n of N") is drawn in the 28pt join inside this rect.
    NSRect imageable = [self currentImageableBounds];
    return NSMakeRect(0, 0, NSWidth(imageable), NSHeight(imageable));
}

- (NSColor *)posterInk {
    return [NSColor colorWithCalibratedRed:0x11 / 255.0
                                     green:0x11 / 255.0
                                      blue:0x11 / 255.0
                                     alpha:1.0];
}

/// Margin-only chrome. Drawn only when this source page split into more
/// than one tile (we created the poster). Hidden when the job is 1 page.
/// Families should not bake "n of N" into a single full-chart PDF — the
/// label here is the post-scale count. Pre-tiled paper-sized pages do not
/// split at Fit/100%, so we will not double their baked chrome.
- (void)drawPosterChromeForTile:(const PerfectPrintPosterTile *)tile
                           grid:(PerfectPrintPosterGrid)grid
                    pageNumber:(NSInteger)pageNumber
                     totalPages:(NSUInteger)totalPages
                    contentRect:(NSRect)contentInView {
    if (!tile || grid.pages <= 1 || totalPages <= 1) {
        return;
    }

    NSColor *ink = [self posterInk];
    CGFloat tick = (CGFloat)PERFECT_PRINT_TICK_LENGTH_PT;
    if (tick > PERFECT_PRINT_TICK_MAX_PT) {
        tick = (CGFloat)PERFECT_PRINT_TICK_MAX_PT;
    }
    CGFloat cx = NSMinX(contentInView);
    CGFloat cy = NSMinY(contentInView);
    CGFloat cw = NSWidth(contentInView);
    CGFloat ch = NSHeight(contentInView);

    [ink setStroke];
    // Right-edge tick when another column joins to the right.
    if (tile->col + 1 < grid.cols) {
        NSBezierPath *path = [NSBezierPath bezierPath];
        path.lineWidth = 0.4;
        CGFloat midY = cy + (ch - tick) * 0.5;
        [path moveToPoint:NSMakePoint(cx + cw + 1.0, midY)];
        [path lineToPoint:NSMakePoint(cx + cw + 1.0 + tick, midY)];
        [path stroke];
    }
    // Bottom-edge tick when another row joins below (lower y, view is y-up).
    if (tile->row + 1 < grid.rows) {
        NSBezierPath *path = [NSBezierPath bezierPath];
        path.lineWidth = 0.4;
        CGFloat midX = cx + (cw - tick) * 0.5;
        [path moveToPoint:NSMakePoint(midX, cy - 1.0)];
        [path lineToPoint:NSMakePoint(midX + tick, cy - 1.0)];
        [path stroke];
    }

    NSFont *font = [NSFont fontWithName:@"Helvetica" size:PERFECT_PRINT_SHEET_LABEL_SIZE];
    if (!font) {
        font = [NSFont systemFontOfSize:PERFECT_PRINT_SHEET_LABEL_SIZE];
    }
    NSDictionary *attrs = @{
        NSFontAttributeName: font,
        NSForegroundColorAttributeName: ink,
    };
    NSString *label = [NSString stringWithFormat:@"%ld of %lu",
                       (long)pageNumber, (unsigned long)totalPages];
    NSSize labelSize = [label sizeWithAttributes:attrs];
    NSPoint point = NSMakePoint(
        cx + cw - labelSize.width,
        cy - 8.0 - labelSize.height);
    if (point.x < cx) {
        point.x = cx;
    }
    [label drawAtPoint:point withAttributes:attrs];
}

- (void)drawRect:(NSRect)dirtyRect {
    (void)dirtyRect;
    NSPrintOperation *operation = NSPrintOperation.currentOperation;
    NSInteger pageNumber = operation ? operation.currentPage : 1;

    NSUInteger documentPage1 = 0;
    uint32_t tileIndex = 0;
    PerfectPrintPosterGrid grid;
    NSRect media;
    memset(&grid, 0, sizeof(grid));
    if (![self resolvePrintPage:pageNumber
                  documentPage:&documentPage1
                     tileIndex:&tileIndex
                          grid:&grid
                         media:&media]) {
        return;
    }

    PDFPage *page = [self.document pageAtIndex:documentPage1 - 1];
    CGFloat scale = [self scaleForMedia:media];
    NSRect content = [self contentBounds];
    NSRect imageable = [self currentImageableBounds];
    NSRect contentInView = NSMakeRect(
        NSMinX(content) - NSMinX(imageable),
        NSMinY(content) - NSMinY(imageable),
        NSWidth(content),
        NSHeight(content));

    PerfectPrintPosterTile tile;
    if (!PerfectPrintPosterTileAt(
            tileIndex,
            NSWidth(media),
            NSHeight(media),
            scale,
            NSWidth(content),
            NSHeight(content),
            &tile)) {
        return;
    }

    // --- AppKit coordinate contract (read this before changing anything
    // below) ---
    // By the time -drawRect: runs, AppKit has already taken the rect we
    // returned from -rectForPage: (the full imageable size) and mapped it
    // onto the paper's imageable area -- applying
    // printInfo.horizontallyCentered / verticallyCentered. So (0,0) here
    // is the imageable origin. Do NOT re-apply imageable-origin or
    // centering math; that would double-offset content.
    //
    // Poster pages: each print page is a contiguous crop of the source
    // PDF page (not a copy of the whole page). Remainder tiles stay
    // top-left aligned inside the content rect so adjacent sheets join.
    CGContextRef context = NSGraphicsContext.currentContext.CGContext;
    CGContextSaveGState(context);

    // Top-left of the content cell in y-up view space. Remainder tiles
    // leave empty space on the right and/or bottom (visual).
    CGFloat destX = NSMinX(contentInView);
    CGFloat destY = NSMinY(contentInView) + (NSHeight(contentInView) - tile.dest_h);
    NSRect dest = NSMakeRect(destX, destY, tile.dest_w, tile.dest_h);
    CGContextClipToRect(context, dest);

    // tile.src_* is top-down in media-relative space. PDF y is up.
    CGFloat pdfX = NSMinX(media) + tile.src_x;
    CGFloat pdfY = NSMinY(media) + NSHeight(media) - tile.src_y - tile.src_h;
    CGContextTranslateCTM(context, destX - pdfX * scale, destY - pdfY * scale);
    CGContextScaleCTM(context, scale, scale);
    [page drawWithBox:kPDFDisplayBoxMediaBox toContext:context];
    CGContextRestoreGState(context);

    NSUInteger totalPages = [self currentPrintPageCount];
    [self drawPosterChromeForTile:&tile
                             grid:grid
                       pageNumber:pageNumber
                       totalPages:totalPages
                      contentRect:contentInView];
}

@end

/// Standard NSPrintPanel options for interactive jobs.
///
/// The system panel (not a custom sheet) then shows Printer, Presets,
/// Copies, Pages, Paper Size, Orientation, Scale, Preview, Page Setup,
/// and the PDF menu. Two-sided/duplex is not a dedicated options bit;
/// AppKit surfaces it on the Copies row when the destination printer
/// supports it (`ShowsCopies` plus `PMSetDuplex`). `NSPrintTwoSided` is
/// not used: it is undeclared on current SDKs (MacOSX15.4 / MacOSX26).
/// Color vs B&W is left to the printer's own system controls.
static NSPrintPanelOptions PerfectPrintStandardPanelOptions(void) {
    return NSPrintPanelShowsCopies
        | NSPrintPanelShowsPageRange
        | NSPrintPanelShowsPaperSize
        | NSPrintPanelShowsOrientation
        | NSPrintPanelShowsScaling
        | NSPrintPanelShowsPreview
        | NSPrintPanelShowsPageSetupAccessory;
}

/// Apply PrintSettings paper/orientation as *defaults* only.
///
/// Sets `paperSize` and orientation from the requested dimensions.
/// `-[NSPrinter paperList]` is not called: that property is absent from
/// current SDKs (MacOSX15.4 / MacOSX26) and `NSPrinter` does not respond
/// to it at runtime. `ShowsPaperSize` still lets the user pick a named
/// printer paper in the panel. Orientation is re-applied after
/// `paperSize` because selecting a paper can reset it.
static void PerfectPrintApplyDefaultPaper(NSPrintInfo *info, NSSize requested, bool landscape) {
    NSPaperOrientation orientation =
        landscape ? NSPaperOrientationLandscape : NSPaperOrientationPortrait;
    info.orientation = orientation;

    if (!(requested.width > 0.0 && requested.height > 0.0 &&
          isfinite(requested.width) && isfinite(requested.height))) {
        return;
    }

    info.paperSize = requested;
    info.orientation = orientation;
}

static void PerfectPrintConfigurePrintInfo(NSPrintInfo *info, PerfectPrintNativeSettings settings) {
    PerfectPrintApplyDefaultPaper(
        info,
        NSMakeSize(settings.paper_width, settings.paper_height),
        settings.landscape);

    info.horizontallyCentered = YES;
    info.verticallyCentered = YES;
    // Automatic — do not force Clip. Clip discarded the panel Scale field
    // for this knowsPageRange:/rectForPage: view. Fit-to-page remains the
    // default content path via PerfectPrintPDFView.scalingMode (mode 0).
    // Page count is owned by the view (poster tiles from Scale).
    info.horizontalPagination = NSPrintingPaginationModeAutomatic;
    info.verticalPagination = NSPrintingPaginationModeAutomatic;

    if (settings.scaling == 3 && isfinite(settings.custom_scale) && settings.custom_scale > 0.0) {
        info.scalingFactor = settings.custom_scale;
    }

    info.dictionary[NSPrintCopies] = @(MAX(settings.copies, 1));
    info.dictionary[NSPrintMustCollate] = @(settings.collate);

    PMPrintSettings pmSettings = (PMPrintSettings)info.PMPrintSettings;
    PMSetCopies(pmSettings, MAX(settings.copies, 1), false);
    PMSetCollate(pmSettings, settings.collate);
    PMDuplexMode duplex = kPMDuplexNone;
    if (settings.duplex == 1) duplex = kPMDuplexNoTumble;
    if (settings.duplex == 2) duplex = kPMDuplexTumble;
    PMSetDuplex(pmSettings, duplex);
    if (settings.color_mode == 0) {
        PMPrintSettingsSetValue(pmSettings, CFSTR("ColorModel"), CFSTR("RGB"), false);
        PMPrintSettingsSetValue(pmSettings, CFSTR("OutputMode"), CFSTR("Color"), false);
    } else {
        PMPrintSettingsSetValue(pmSettings, CFSTR("ColorModel"), CFSTR("Gray"), false);
        PMPrintSettingsSetValue(pmSettings, CFSTR("OutputMode"), CFSTR("Grayscale"), false);
    }
    [info updateFromPMPrintSettings];
}

static void PerfectPrintConfigurePrintOperation(NSPrintOperation *operation, const char *titleUtf8) {
    if (titleUtf8) {
        NSString *title = [NSString stringWithUTF8String:titleUtf8];
        if (title.length > 0) operation.jobTitle = title;
    }
    operation.showsPrintPanel = YES;
    operation.showsProgressPanel = YES;
    NSPrintPanel *panel = operation.printPanel;
    panel.options = panel.options | PerfectPrintStandardPanelOptions();
}

static int32_t perfect_print_run_pdf_dialog(
    const uint8_t *pdfBytes,
    size_t pdfLength,
    const char *titleUtf8,
    PerfectPrintNativeSettings settings,
    const uint32_t *selectedPages,
    size_t selectedPageCount
) {
    @autoreleasepool {
        NSData *data = [NSData dataWithBytes:pdfBytes length:pdfLength];
        PDFDocument *document = [[PDFDocument alloc] initWithData:data];
        if (!document || document.pageCount == 0) {
            return -1;
        }

        NSUInteger documentPageCount = document.pageCount;

        // Pagination is poster tiles of each source page, not one print
        // page per PDF page. -rectForPage: returns the imageable rect
        // (one sheet). Size the frame to a generous paper so Tabloid /
        // A3 still fit; do not size to the full chart — a huge frame
        // plus Automatic pagination would invent extra slices.
        CGFloat frameW = MAX(settings.paper_width, 1224.0);
        CGFloat frameH = MAX(settings.paper_height, 1224.0);
        if (!(frameW > 0.0) || !isfinite(frameW)) frameW = 1224.0;
        if (!(frameH > 0.0) || !isfinite(frameH)) frameH = 1224.0;
        NSRect frame = NSMakeRect(0, 0, frameW, frameH);

        PerfectPrintPDFView *view = [[PerfectPrintPDFView alloc] initWithFrame:frame];
        view.document = document;
        view.fallbackPaperSize = NSMakeSize(settings.paper_width, settings.paper_height);
        // Custom(scale) is seeded onto printInfo.scalingFactor so the
        // panel Scale field shows that percentage and can change it.
        // Other modes keep their view-side default (FitToPage is 0).
        if (settings.scaling == 3) {
            view.scalingMode = 2;
            view.customScale = 1.0;
        } else {
            view.scalingMode = settings.scaling;
            view.customScale = settings.custom_scale;
        }

        NSMutableArray<NSNumber *> *pageNumbers = [NSMutableArray array];
        if (settings.page_range_kind == 1) {
            NSUInteger first = MAX((NSUInteger)settings.first_page, 1);
            NSUInteger last = MIN((NSUInteger)settings.last_page, documentPageCount);
            if (first > last) return -1;
            for (NSUInteger page = first; page <= last; page++) [pageNumbers addObject:@(page)];
        } else if (settings.page_range_kind == 2) {
            for (size_t index = 0; index < selectedPageCount; index++) {
                NSUInteger page = selectedPages[index];
                if (page >= 1 && page <= documentPageCount && ![pageNumbers containsObject:@(page)]) {
                    [pageNumbers addObject:@(page)];
                }
            }
            if (pageNumbers.count == 0) return -1;
        } else {
            for (NSUInteger page = 1; page <= documentPageCount; page++) [pageNumbers addObject:@(page)];
        }
        view.pageNumbers = pageNumbers;

        NSPrintInfo *info = [[NSPrintInfo sharedPrintInfo] copy];
        PerfectPrintConfigurePrintInfo(info, settings);

        NSPrintOperation *operation = [NSPrintOperation printOperationWithView:view printInfo:info];
        PerfectPrintConfigurePrintOperation(operation, titleUtf8);
        BOOL accepted = [operation runOperation];
        return accepted ? 1 : 0;
    }
}

int32_t perfect_print_pdf_dialog(
    const uint8_t *pdfBytes,
    size_t pdfLength,
    const char *titleUtf8,
    PerfectPrintNativeSettings settings,
    const uint32_t *selectedPages,
    size_t selectedPageCount
) {
    if (!pdfBytes || pdfLength < 5) {
        return -1;
    }

    if (NSThread.isMainThread) {
        return perfect_print_run_pdf_dialog(
            pdfBytes, pdfLength, titleUtf8, settings, selectedPages, selectedPageCount);
    }

    __block int32_t result = -1;
    dispatch_sync(dispatch_get_main_queue(), ^{
        result = perfect_print_run_pdf_dialog(
            pdfBytes, pdfLength, titleUtf8, settings, selectedPages, selectedPageCount);
    });
    return result;
}

uint32_t perfect_print_native_panel_options_mask(void) {
    return (uint32_t)PerfectPrintStandardPanelOptions();
}

/// Configure a throwaway `NSPrintOperation` the same way interactive jobs
/// do and read back `printPanel.options` plus pagination. Does not run the
/// operation or show a sheet — used by Rust tests on macOS.
int32_t perfect_print_native_inspect_panel_defaults(
    uint32_t *out_options,
    uint32_t *out_horizontal_pagination,
    uint32_t *out_vertical_pagination
) {
    @autoreleasepool {
        PerfectPrintNativeSettings settings;
        memset(&settings, 0, sizeof(settings));
        settings.copies = 1;
        settings.paper_width = 612.0;
        settings.paper_height = 792.0;
        settings.collate = true;

        NSPrintInfo *info = [[NSPrintInfo sharedPrintInfo] copy];
        PerfectPrintConfigurePrintInfo(info, settings);

        NSView *view = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 100, 100)];
        NSPrintOperation *operation = [NSPrintOperation printOperationWithView:view printInfo:info];
        PerfectPrintConfigurePrintOperation(operation, "Perfect Print");

        if (out_options) {
            *out_options = (uint32_t)operation.printPanel.options;
        }
        if (out_horizontal_pagination) {
            *out_horizontal_pagination = (uint32_t)operation.printInfo.horizontalPagination;
        }
        if (out_vertical_pagination) {
            *out_vertical_pagination = (uint32_t)operation.printInfo.verticalPagination;
        }
        return 1;
    }
}
