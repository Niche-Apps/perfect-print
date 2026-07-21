#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <Quartz/Quartz.h>
#import <ApplicationServices/ApplicationServices.h>
#include <stdbool.h>
#include <stdint.h>
#include <dispatch/dispatch.h>

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
@end

@implementation PerfectPrintPDFView

- (BOOL)knowsPageRange:(NSRangePointer)range {
    if (!self.document || self.document.pageCount == 0) {
        return NO;
    }
    range->location = 1;
    range->length = self.pageNumbers.count;
    return YES;
}

/// Media box (in PDF page space) for the given 1-based print-operation page
/// number, or `self.bounds` as a last-resort fallback if the page number is
/// out of range (mirrors the pre-existing behavior of `rectForPage:`).
- (NSRect)mediaForPage:(NSInteger)pageNumber {
    if (pageNumber < 1 || pageNumber > (NSInteger)self.pageNumbers.count) {
        return self.bounds;
    }
    NSUInteger documentIndex = self.pageNumbers[(NSUInteger)(pageNumber - 1)].unsignedIntegerValue - 1;
    PDFPage *page = [self.document pageAtIndex:documentIndex];
    return [page boundsForBox:kPDFDisplayBoxMediaBox];
}

/// The scale factor this view applies to `media` under the current scaling
/// mode. Shared by `rectForPage:` and `drawRect:` so the size AppKit is told
/// to place (via `rectForPage:`) and the size we actually draw at (in
/// `drawRect:`) can never drift apart.
///
/// Reads the current print operation's `imageablePageBounds` to compute the
/// FitToPage/FillPage ratios; if there is no current operation (e.g. this
/// view is being sized outside of an active `NSPrintOperation`), falls back
/// to scale 1.0 rather than guessing.
- (CGFloat)scaleForMedia:(NSRect)media {
    NSPrintOperation *operation = NSPrintOperation.currentOperation;
    if (!operation) {
        return 1.0;
    }
    NSRect imageable = operation.printInfo.imageablePageBounds;
    CGFloat sx = NSWidth(imageable) / MAX(NSWidth(media), 1.0);
    CGFloat sy = NSHeight(imageable) / MAX(NSHeight(media), 1.0);
    switch (self.scalingMode) {
        case 0: return MIN(sx, sy);                    // FitToPage
        case 1: return MAX(sx, sy);                     // FillPage
        case 2: return 1.0;                              // None
        case 3: return MAX(self.customScale, 0.01);      // Custom
        default: return MIN(sx, sy);
    }
}

- (NSRect)rectForPage:(NSInteger)pageNumber {
    NSRect media = [self mediaForPage:pageNumber];
    CGFloat scale = [self scaleForMedia:media];
    return NSMakeRect(0, 0, NSWidth(media) * scale, NSHeight(media) * scale);
}

- (void)drawRect:(NSRect)dirtyRect {
    (void)dirtyRect;
    NSPrintOperation *operation = NSPrintOperation.currentOperation;
    NSInteger pageNumber = operation ? operation.currentPage : 1;
    if (pageNumber < 1 || pageNumber > (NSInteger)self.pageNumbers.count) {
        return;
    }

    NSUInteger documentIndex = self.pageNumbers[(NSUInteger)(pageNumber - 1)].unsignedIntegerValue - 1;
    PDFPage *page = [self.document pageAtIndex:documentIndex];
    NSRect media = [page boundsForBox:kPDFDisplayBoxMediaBox];
    CGFloat scale = [self scaleForMedia:media];

    // --- AppKit coordinate contract (read this before changing anything
    // below) ---
    // By the time -drawRect: runs, AppKit has already taken the rect we
    // returned from -rectForPage: and mapped it onto the paper's imageable
    // area for us -- applying printInfo.horizontallyCentered/
    // verticallyCentered and the imageable origin itself. So the view's
    // coordinate space *inside this method* is already page space: (0,0)
    // here is the origin of the (scaled) rect -rectForPage: returned, and
    // AppKit has already positioned/centered that rect within the
    // imageable area. This method must therefore draw entirely within
    // [0, rectForPage:'s size] and must NOT re-derive or re-apply the
    // imageable origin or any centering math here -- that would double
    // apply AppKit's own placement on top of what -rectForPage: already
    // told it to do.
    //
    // That double-application was the actual bug: this method used to
    // translate by `NSMinX(imageable) + (NSWidth(imageable) -
    // renderedWidth) / 2.0` (and the y equivalent), stacking a second
    // imageable-origin/centering offset on top of the one AppKit had
    // already applied via -rectForPage:. With Letter media on A4 paper at
    // Custom(1.0) scale, that pushed content to x = -8.4pt, clipping the
    // left ~8pt of every printed page.
    //
    // The only things this method owns: (1) the scale factor -- which
    // MUST match -rectForPage:'s (both call -scaleForMedia:, so they
    // can't drift), and (2) translating the PDF's own MediaBox origin
    // (not necessarily (0,0) for every PDF) to this view's (0,0).
    CGContextRef context = NSGraphicsContext.currentContext.CGContext;
    CGContextSaveGState(context);
    CGContextScaleCTM(context, scale, scale);
    CGContextTranslateCTM(context, -NSMinX(media), -NSMinY(media));
    [page drawWithBox:kPDFDisplayBoxMediaBox toContext:context];
    CGContextRestoreGState(context);
}

@end

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

        // AppKit requires that every rect -rectForPage: returns fit within
        // the view's own bounds. -rectForPage: (see above) returns the
        // page's media box scaled by -scaleForMedia:, but at this point we
        // don't yet have a live NSPrintOperation (it's created below, after
        // the view), so -scaleForMedia:'s FitToPage/FillPage ratios --
        // which depend on operation.printInfo.imageablePageBounds -- aren't
        // computable yet. We approximate conservatively instead:
        //   - FitToPage/FillPage/None (modes 0/1/2): these scale by a ratio
        //     computed against the *actual* printer's imageable bounds,
        //     which are typically <= the requested paper size, so ratios
        //     are usually <= 1. We size the frame to the largest page's raw
        //     media size (multiplier 1.0), matching the pre-existing
        //     behavior for the (previously page-1-only) frame.
        //   - Custom (mode 3): the multiplier is known up front (it's the
        //     user-requested custom_scale), so size the frame to the
        //     largest media size times max(1.0, custom_scale) -- covering
        //     both "shrink" (< 1.0, where raw media size already suffices)
        //     and "enlarge" (> 1.0, where the scaled page is bigger than
        //     any single page's own media box) cases.
        // If a real printer's imageable bounds ever produce a FitToPage/
        // FillPage ratio > 1 (unusual, but possible for tiny custom paper
        // sizes), AppKit will clip that page's content to the view bounds
        // rather than crash; this is a sizing approximation, not a
        // correctness-critical computation.
        CGFloat frameScaleMultiplier =
            (settings.scaling == 3) ? MAX(settings.custom_scale, 1.0) : 1.0;
        CGFloat maxMediaWidth = 1.0;
        CGFloat maxMediaHeight = 1.0;
        for (NSUInteger i = 0; i < documentPageCount; i++) {
            NSRect pageMedia = [[document pageAtIndex:i] boundsForBox:kPDFDisplayBoxMediaBox];
            maxMediaWidth = MAX(maxMediaWidth, NSWidth(pageMedia));
            maxMediaHeight = MAX(maxMediaHeight, NSHeight(pageMedia));
        }
        NSRect frame = NSMakeRect(
            0, 0,
            maxMediaWidth * frameScaleMultiplier,
            maxMediaHeight * frameScaleMultiplier);

        PerfectPrintPDFView *view = [[PerfectPrintPDFView alloc] initWithFrame:frame];
        view.document = document;
        view.scalingMode = settings.scaling;
        view.customScale = settings.custom_scale;

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
        NSSize requestedPaper = NSMakeSize(settings.paper_width, settings.paper_height);

        if (requestedPaper.width > 0.0 && requestedPaper.height > 0.0 &&
            isfinite(requestedPaper.width) && isfinite(requestedPaper.height)) {
            info.paperSize = requestedPaper;
        }
        info.orientation = settings.landscape ? NSPaperOrientationLandscape : NSPaperOrientationPortrait;
        info.horizontallyCentered = YES;
        info.verticallyCentered = YES;
        info.horizontalPagination = NSPrintingPaginationModeClip;
        info.verticalPagination = NSPrintingPaginationModeClip;
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

        NSPrintOperation *operation = [NSPrintOperation printOperationWithView:view printInfo:info];
        if (titleUtf8) {
            NSString *title = [NSString stringWithUTF8String:titleUtf8];
            if (title.length > 0) operation.jobTitle = title;
        }
        operation.showsPrintPanel = YES;
        operation.showsProgressPanel = YES;
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
