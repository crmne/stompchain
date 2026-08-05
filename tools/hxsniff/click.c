/* Minimal synthetic-click helper: click.c x y [count]
 *
 * HX Edit draws its own UI, so the accessibility tree exposes nothing but the
 * window chrome and we cannot address controls by name. Clicking at absolute
 * screen coordinates is the remaining option. Requires the calling terminal to
 * hold Accessibility permission.
 */
#include <ApplicationServices/ApplicationServices.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: click x y [count]\n");
        return 2;
    }
    double x = atof(argv[1]), y = atof(argv[2]);
    int count = argc > 3 ? atoi(argv[3]) : 1;
    CGPoint pt = CGPointMake(x, y);

    /* Move first and let the app settle: many custom UIs only update their hit
     * target on mouse-move, and a click with no prior move can land on a stale
     * region. */
    CGEventRef mv = CGEventCreateMouseEvent(NULL, kCGEventMouseMoved, pt,
                                            kCGMouseButtonLeft);
    CGEventPost(kCGHIDEventTap, mv);
    CFRelease(mv);
    usleep(120000);

    for (int i = 0; i < count; i++) {
        CGEventRef down = CGEventCreateMouseEvent(NULL, kCGEventLeftMouseDown,
                                                  pt, kCGMouseButtonLeft);
        CGEventRef up = CGEventCreateMouseEvent(NULL, kCGEventLeftMouseUp, pt,
                                                kCGMouseButtonLeft);
        if (count > 1) {
            CGEventSetIntegerValueField(down, kCGMouseEventClickState, i + 1);
            CGEventSetIntegerValueField(up, kCGMouseEventClickState, i + 1);
        }
        CGEventPost(kCGHIDEventTap, down);
        usleep(30000);
        CGEventPost(kCGHIDEventTap, up);
        CFRelease(down);
        CFRelease(up);
        usleep(60000);
    }
    return 0;
}
