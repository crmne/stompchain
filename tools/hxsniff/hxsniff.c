/*
 * hxsniff — libusb interposer for capturing HX Edit's USB traffic.
 *
 * HX Edit links a bundled copy of libusb (@executable_path/../MacOS/
 * libusb-1.0.0.dylib), so every byte it exchanges with the device passes
 * through a handful of libusb entry points. Interposing those gives us the
 * protocol at API level — complete buffers, correct boundaries, no reassembly
 * — which is strictly better than a packet capture.
 *
 * Build and use: see run.sh in this directory.
 */

#include <libusb-1.0/libusb.h>

#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define DYLD_INTERPOSE(_replacement, _replacee)                                \
    __attribute__((used)) static struct {                                      \
        const void *replacement;                                               \
        const void *replacee;                                                  \
    } _interpose_##_replacee __attribute__((section("__DATA,__interpose"))) = { \
        (const void *)(unsigned long)&_replacement,                            \
        (const void *)(unsigned long)&_replacee};

/* ------------------------------------------------------------------ log --- */

static FILE *g_log;
static pthread_mutex_t g_lock = PTHREAD_MUTEX_INITIALIZER;
static unsigned long g_seq;
static double g_t0;

static double now_sec(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}

__attribute__((constructor)) static void hxsniff_init(void)
{
    const char *path = getenv("HXSNIFF_LOG");
    if (!path)
        path = "/tmp/hxsniff.log";
    g_log = fopen(path, "w");
    if (!g_log)
        g_log = stderr;
    setvbuf(g_log, NULL, _IOLBF, 0);
    g_t0 = now_sec();
    fprintf(g_log, "# hxsniff attached pid=%d\n", getpid());
}

/* A marker line can be injected by the driver script (see mark()) so captures
 * can be sliced by UI action afterwards. We poll a file rather than use a
 * signal to keep this async-signal-safety-free. */
static void drain_marks(void)
{
    const char *mp = getenv("HXSNIFF_MARK");
    if (!mp)
        return;
    FILE *f = fopen(mp, "r");
    if (!f)
        return;
    char buf[512];
    while (fgets(buf, sizeof buf, f))
        fprintf(g_log, "### MARK %s", buf);
    fclose(f);
    unlink(mp);
}

static void hexdump(const unsigned char *p, int n)
{
    for (int i = 0; i < n; i += 16) {
        fprintf(g_log, "\t%04x  ", i);
        for (int j = 0; j < 16; j++) {
            if (i + j < n)
                fprintf(g_log, "%02x ", p[i + j]);
            else
                fprintf(g_log, "   ");
            if (j == 7)
                fputc(' ', g_log);
        }
        fputc('|', g_log);
        for (int j = 0; j < 16 && i + j < n; j++) {
            unsigned char c = p[i + j];
            fputc((c >= 0x20 && c < 0x7f) ? c : '.', g_log);
        }
        fprintf(g_log, "|\n");
    }
}

static void emit(const char *kind, int ep, const unsigned char *data, int len,
                 const char *extra)
{
    pthread_mutex_lock(&g_lock);
    drain_marks();
    fprintf(g_log, "[%lu] +%.6f %s ep=0x%02x len=%d%s%s\n", g_seq++,
            now_sec() - g_t0, kind, ep, len, extra ? " " : "",
            extra ? extra : "");
    if (data && len > 0)
        hexdump(data, len);
    fflush(g_log);
    pthread_mutex_unlock(&g_lock);
}

static void note(const char *fmt, ...)
{
    va_list ap;
    pthread_mutex_lock(&g_lock);
    drain_marks();
    fprintf(g_log, "[%lu] +%.6f ", g_seq++, now_sec() - g_t0);
    va_start(ap, fmt);
    vfprintf(g_log, fmt, ap);
    va_end(ap);
    fputc('\n', g_log);
    fflush(g_log);
    pthread_mutex_unlock(&g_lock);
}

/* ------------------------------------------- async callback redirection --- */

/* Transfers are typically allocated once and resubmitted, so we permanently
 * swap in our trampoline and keep the app's callback in a side table keyed by
 * transfer pointer. */
#define MAPCAP 4096
static struct {
    struct libusb_transfer *key;
    libusb_transfer_cb_fn cb;
} g_map[MAPCAP];
static pthread_mutex_t g_maplock = PTHREAD_MUTEX_INITIALIZER;

static size_t map_slot(struct libusb_transfer *t)
{
    return ((size_t)(uintptr_t)t >> 4) % MAPCAP;
}

static void map_put(struct libusb_transfer *t, libusb_transfer_cb_fn cb)
{
    size_t i = map_slot(t);
    for (size_t n = 0; n < MAPCAP; n++, i = (i + 1) % MAPCAP) {
        if (g_map[i].key == NULL || g_map[i].key == t) {
            g_map[i].key = t;
            g_map[i].cb = cb;
            return;
        }
    }
}

static libusb_transfer_cb_fn map_get(struct libusb_transfer *t)
{
    size_t i = map_slot(t);
    for (size_t n = 0; n < MAPCAP; n++, i = (i + 1) % MAPCAP) {
        if (g_map[i].key == t)
            return g_map[i].cb;
        if (g_map[i].key == NULL)
            return NULL;
    }
    return NULL;
}

static void map_del(struct libusb_transfer *t)
{
    size_t i = map_slot(t);
    for (size_t n = 0; n < MAPCAP; n++, i = (i + 1) % MAPCAP) {
        if (g_map[i].key == t) {
            g_map[i].key = NULL;
            g_map[i].cb = NULL;
            return;
        }
    }
}

static const char *ttype(unsigned char t)
{
    switch (t) {
    case LIBUSB_TRANSFER_TYPE_CONTROL:     return "CTRL";
    case LIBUSB_TRANSFER_TYPE_ISOCHRONOUS: return "ISOC";
    case LIBUSB_TRANSFER_TYPE_BULK:        return "BULK";
    case LIBUSB_TRANSFER_TYPE_INTERRUPT:   return "INTR";
    default:                               return "????";
    }
}

/* Rewrite a reply before HX Edit sees it.
 *
 * Set HXSNIFF_PATCH to a hex pattern and its replacement, separated by a
 * slash: `HXSNIFF_PATCH=68813fc2/68813fc3` flips the single-key boolean in
 * an opcode-99 reply from false to true. Only what the *application* sees
 * changes; the bytes on the wire to the device are never touched.
 *
 * The point of this is to identify a flag by its effect: a value nothing acts
 * on cannot be distinguished from any other, so the way to learn what a flag
 * means is to change it and watch the client.
 */
static void patch_in(unsigned char *buf, int len)
{
    static unsigned char find[16], repl[16];
    static int n = -1;
    if (n < 0) {
        const char *spec = getenv("HXSNIFF_PATCH");
        n = 0;
        if (spec) {
            const char *slash = strchr(spec, '/');
            if (slash) {
                int i;
                for (i = 0; i < 16 && spec + 2 * i + 1 < slash; i++)
                    sscanf(spec + 2 * i, "%2hhx", &find[i]);
                n = i;
                for (i = 0; i < n; i++)
                    sscanf(slash + 1 + 2 * i, "%2hhx", &repl[i]);
            }
        }
    }
    if (n <= 0 || !buf || len < n)
        return;
    for (int i = 0; i + n <= len; i++) {
        if (memcmp(buf + i, find, n) == 0) {
            memcpy(buf + i, repl, n);
            emit("PATCHED", 0x81, buf + i, n, "reply rewritten");
        }
    }
}

static void trampoline(struct libusb_transfer *t)
{
    char extra[96];
    int in = (t->endpoint & 0x80) != 0;

    if (in && t->actual_length > 0)
        patch_in(t->buffer, t->actual_length);

    snprintf(extra, sizeof extra, "status=%d actual=%d", t->status,
             t->actual_length);
    /* Only IN data is new information at completion time; OUT payloads were
     * already logged at submit. */
    emit(in ? "ASYNC-IN-DONE" : "ASYNC-OUT-DONE", t->endpoint,
         in ? t->buffer : NULL, in ? t->actual_length : 0, extra);

    pthread_mutex_lock(&g_maplock);
    libusb_transfer_cb_fn orig = map_get(t);
    pthread_mutex_unlock(&g_maplock);
    if (orig)
        orig(t);
}

/* --------------------------------------------------------- interposers --- */

static int my_libusb_submit_transfer(struct libusb_transfer *t)
{
    char extra[128];
    int in = (t->endpoint & 0x80) != 0;

    snprintf(extra, sizeof extra, "type=%s %s", ttype(t->type),
             in ? "(in, awaiting data)" : "");
    emit(in ? "ASYNC-IN-SUBMIT" : "ASYNC-OUT", t->endpoint,
         in ? NULL : t->buffer, in ? 0 : t->length, extra);

    pthread_mutex_lock(&g_maplock);
    if (t->callback != trampoline) {
        map_put(t, t->callback);
        t->callback = trampoline;
    }
    pthread_mutex_unlock(&g_maplock);

    return libusb_submit_transfer(t);
}

static void my_libusb_free_transfer(struct libusb_transfer *t)
{
    if (t) {
        pthread_mutex_lock(&g_maplock);
        map_del(t);
        pthread_mutex_unlock(&g_maplock);
    }
    libusb_free_transfer(t);
}

static int my_libusb_bulk_transfer(libusb_device_handle *h, unsigned char ep,
                                   unsigned char *data, int length,
                                   int *actual_length, unsigned int timeout)
{
    int in = (ep & 0x80) != 0;
    if (!in)
        emit("BULK-OUT", ep, data, length, NULL);

    int rc = libusb_bulk_transfer(h, ep, data, length, actual_length, timeout);

    if (in) {
        char extra[64];
        int n = actual_length ? *actual_length : 0;
        snprintf(extra, sizeof extra, "rc=%d", rc);
        emit("BULK-IN", ep, rc == 0 ? data : NULL, rc == 0 ? n : 0, extra);
    } else {
        note("BULK-OUT-DONE ep=0x%02x rc=%d actual=%d", ep, rc,
             actual_length ? *actual_length : -1);
    }
    return rc;
}

static int my_libusb_interrupt_transfer(libusb_device_handle *h,
                                        unsigned char ep, unsigned char *data,
                                        int length, int *actual_length,
                                        unsigned int timeout)
{
    int in = (ep & 0x80) != 0;
    if (!in)
        emit("INTR-OUT", ep, data, length, NULL);

    int rc = libusb_interrupt_transfer(h, ep, data, length, actual_length,
                                       timeout);

    if (in) {
        char extra[64];
        int n = actual_length ? *actual_length : 0;
        snprintf(extra, sizeof extra, "rc=%d", rc);
        emit("INTR-IN", ep, rc == 0 ? data : NULL, rc == 0 ? n : 0, extra);
    }
    return rc;
}

static int my_libusb_control_transfer(libusb_device_handle *h, uint8_t rtype,
                                      uint8_t req, uint16_t val, uint16_t idx,
                                      unsigned char *data, uint16_t len,
                                      unsigned int timeout)
{
    char extra[160];
    int in = (rtype & 0x80) != 0;

    snprintf(extra, sizeof extra,
             "bmRequestType=0x%02x bRequest=0x%02x wValue=0x%04x wIndex=0x%04x",
             rtype, req, val, idx);
    if (!in)
        emit("CTRL-OUT", 0, data, len, extra);

    int rc = libusb_control_transfer(h, rtype, req, val, idx, data, len,
                                     timeout);

    if (in) {
        char e2[200];
        snprintf(e2, sizeof e2, "%s rc=%d", extra, rc);
        emit("CTRL-IN", 0, rc > 0 ? data : NULL, rc > 0 ? rc : 0, e2);
    } else {
        note("CTRL-OUT-DONE rc=%d %s", rc, extra);
    }
    return rc;
}

static int my_libusb_claim_interface(libusb_device_handle *h, int iface)
{
    int rc = libusb_claim_interface(h, iface);
    note("CLAIM interface=%d rc=%d (%s)", iface, rc, libusb_error_name(rc));
    return rc;
}

static int my_libusb_release_interface(libusb_device_handle *h, int iface)
{
    note("RELEASE interface=%d", iface);
    return libusb_release_interface(h, iface);
}

static int my_libusb_set_interface_alt_setting(libusb_device_handle *h,
                                               int iface, int alt)
{
    int rc = libusb_set_interface_alt_setting(h, iface, alt);
    note("SET-ALT interface=%d alt=%d rc=%d", iface, alt, rc);
    return rc;
}

static libusb_device_handle *
my_libusb_open_device_with_vid_pid(libusb_context *ctx, uint16_t vid,
                                   uint16_t pid)
{
    libusb_device_handle *h = libusb_open_device_with_vid_pid(ctx, vid, pid);
    note("OPEN-VIDPID %04x:%04x -> %p", vid, pid, (void *)h);
    return h;
}

static int my_libusb_open(libusb_device *dev, libusb_device_handle **h)
{
    int rc = libusb_open(dev, h);
    struct libusb_device_descriptor d;
    if (libusb_get_device_descriptor(dev, &d) == 0)
        note("OPEN %04x:%04x rc=%d", d.idVendor, d.idProduct, rc);
    else
        note("OPEN rc=%d", rc);
    return rc;
}

static int my_libusb_reset_device(libusb_device_handle *h)
{
    note("RESET-DEVICE");
    return libusb_reset_device(h);
}

DYLD_INTERPOSE(my_libusb_submit_transfer, libusb_submit_transfer)
DYLD_INTERPOSE(my_libusb_free_transfer, libusb_free_transfer)
DYLD_INTERPOSE(my_libusb_bulk_transfer, libusb_bulk_transfer)
DYLD_INTERPOSE(my_libusb_interrupt_transfer, libusb_interrupt_transfer)
DYLD_INTERPOSE(my_libusb_control_transfer, libusb_control_transfer)
DYLD_INTERPOSE(my_libusb_claim_interface, libusb_claim_interface)
DYLD_INTERPOSE(my_libusb_release_interface, libusb_release_interface)
DYLD_INTERPOSE(my_libusb_set_interface_alt_setting,
               libusb_set_interface_alt_setting)
DYLD_INTERPOSE(my_libusb_open_device_with_vid_pid,
               libusb_open_device_with_vid_pid)
DYLD_INTERPOSE(my_libusb_open, libusb_open)
DYLD_INTERPOSE(my_libusb_reset_device, libusb_reset_device)
