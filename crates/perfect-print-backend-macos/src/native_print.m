#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <Quartz/Quartz.h>
#import <ApplicationServices/ApplicationServices.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
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
    CGFloat base = MIN(sx, sy); // FitToPage
    switch (self.scalingMode) {
        case 0: base = MIN(sx, sy); break;                 // FitToPage
        case 1: base = MAX(sx, sy); break;                  // FillPage
        case 2: base = 1.0; break;                           // None
        case 3: base = MAX(self.customScale, 0.01); break;   // Custom
        default: break;
    }
    // This view owns pagination via knowsPageRange:/rectForPage:, so the
    // native panel's Scale field (NSPrintInfo.scalingFactor) has to be
    // folded in here — AppKit will not reliably apply it on top of a
    // custom-paginated view, and NSPrintingPaginationModeClip used to
    // discard it entirely. Fit-to-page remains the default `base`.
    CGFloat panelScale = operation.printInfo.scalingFactor;
    if (panelScale <= 0.0 || !isfinite(panelScale)) {
        panelScale = 1.0;
    }
    return base * panelScale;
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

        // AppKit requires that every rect -rectForPage: returns fit within
        // the view's own bounds. -rectForPage: returns the page's media box
        // scaled by -scaleForMedia: (FitToPage/FillPage/None/Custom, then
        // multiplied by the panel's scalingFactor). We don't have a live
        // NSPrintOperation yet, so imageable-bounds ratios aren't known.
        // Size the frame to the largest page times at least 4.0 so the
        // native Scale field can enlarge up to 400% without violating the
        // bounds contract; Custom(scale) uses max(4.0, custom_scale).
        CGFloat frameScaleMultiplier = 4.0;
        if (settings.scaling == 3) {
            frameScaleMultiplier = MAX(frameScaleMultiplier, MAX(settings.custom_scale, 1.0));
        }
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
