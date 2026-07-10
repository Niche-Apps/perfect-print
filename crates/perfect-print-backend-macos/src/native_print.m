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

- (NSRect)rectForPage:(NSInteger)pageNumber {
    if (pageNumber < 1 || pageNumber > (NSInteger)self.pageNumbers.count) {
        return self.bounds;
    }
    NSUInteger documentIndex = self.pageNumbers[(NSUInteger)(pageNumber - 1)].unsignedIntegerValue - 1;
    PDFPage *page = [self.document pageAtIndex:documentIndex];
    NSRect bounds = [page boundsForBox:kPDFDisplayBoxMediaBox];
    return NSMakeRect(0, 0, NSWidth(bounds), NSHeight(bounds));
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
    NSRect imageable = operation ? operation.printInfo.imageablePageBounds : self.bounds;

    CGFloat sx = NSWidth(imageable) / MAX(NSWidth(media), 1.0);
    CGFloat sy = NSHeight(imageable) / MAX(NSHeight(media), 1.0);
    CGFloat scale = 1.0;
    switch (self.scalingMode) {
        case 0: scale = MIN(sx, sy); break;       // FitToPage
        case 1: scale = MAX(sx, sy); break;       // FillPage
        case 2: scale = 1.0; break;               // None
        case 3: scale = MAX(self.customScale, 0.01); break;
        default: scale = MIN(sx, sy); break;
    }

    CGFloat renderedWidth = NSWidth(media) * scale;
    CGFloat renderedHeight = NSHeight(media) * scale;
    CGFloat x = NSMinX(imageable) + (NSWidth(imageable) - renderedWidth) / 2.0;
    CGFloat y = NSMinY(imageable) + (NSHeight(imageable) - renderedHeight) / 2.0;

    CGContextRef context = NSGraphicsContext.currentContext.CGContext;
    CGContextSaveGState(context);
    CGContextTranslateCTM(context, x, y);
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

        PDFPage *firstPage = [document pageAtIndex:0];
        NSRect media = [firstPage boundsForBox:kPDFDisplayBoxMediaBox];
        PerfectPrintPDFView *view = [[PerfectPrintPDFView alloc]
            initWithFrame:NSMakeRect(0, 0, NSWidth(media), NSHeight(media))];
        view.document = document;
        view.scalingMode = settings.scaling;
        view.customScale = settings.custom_scale;

        NSMutableArray<NSNumber *> *pageNumbers = [NSMutableArray array];
        NSUInteger documentPageCount = document.pageCount;
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
