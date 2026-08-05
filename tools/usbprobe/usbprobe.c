// usbprobe — enumerate a USB device's interfaces/endpoints and test whether an
// interface can be claimed from userspace.
//
//   usbprobe                 enumerate the default target (Line 6, VID 0x0E41)
//   usbprobe <vid> <pid>     enumerate a specific device (hex, e.g. 0e41 4246)
//   usbprobe claim <iface>   additionally try to claim an interface and report why not
//
// Claiming is the interesting part: an interface that userspace can claim has no
// kernel driver bound to it, and a claim that fails with LIBUSB_ERROR_ACCESS/BUSY
// while a vendor app is running tells you that app owns the interface.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libusb.h>

#define DEFAULT_VID 0x0e41 /* Line 6 */

static const char *xfer_name(int type) {
    switch (type & LIBUSB_TRANSFER_TYPE_MASK) {
        case LIBUSB_TRANSFER_TYPE_CONTROL:     return "control";
        case LIBUSB_TRANSFER_TYPE_ISOCHRONOUS: return "isochronous";
        case LIBUSB_TRANSFER_TYPE_BULK:        return "bulk";
        case LIBUSB_TRANSFER_TYPE_INTERRUPT:   return "interrupt";
        default:                               return "unknown";
    }
}

static const char *class_name(int cls) {
    switch (cls) {
        case 0x01: return "audio";
        case 0x02: return "comm";
        case 0x03: return "HID";
        case 0x08: return "mass storage";
        case 0x0a: return "CDC data";
        case 0xfe: return "application specific";
        case 0xff: return "vendor specific";
        default:   return "other";
    }
}

static void print_string(libusb_device_handle *h, uint8_t idx, const char *label) {
    unsigned char buf[256];
    if (!h || idx == 0) return;
    if (libusb_get_string_descriptor_ascii(h, idx, buf, sizeof buf) > 0)
        printf("  %-14s %s\n", label, buf);
}

int main(int argc, char **argv) {
    int want_vid = DEFAULT_VID, want_pid = -1;
    int claim_iface = -1;

    if (argc >= 3 && strcmp(argv[1], "claim") == 0) {
        claim_iface = (int)strtol(argv[2], NULL, 10);
    } else if (argc >= 3) {
        want_vid = (int)strtol(argv[1], NULL, 16);
        want_pid = (int)strtol(argv[2], NULL, 16);
    }

    libusb_context *ctx = NULL;
    int rc = libusb_init(&ctx);
    if (rc != 0) {
        fprintf(stderr, "libusb_init failed: %s\n", libusb_error_name(rc));
        return 1;
    }

    libusb_device **list;
    ssize_t n = libusb_get_device_list(ctx, &list);
    int found = 0;

    for (ssize_t i = 0; i < n; i++) {
        struct libusb_device_descriptor dd;
        if (libusb_get_device_descriptor(list[i], &dd) != 0) continue;
        if (dd.idVendor != want_vid) continue;
        if (want_pid >= 0 && dd.idProduct != want_pid) continue;
        found = 1;

        printf("=== device %04x:%04x (bus %d, addr %d) ===\n",
               dd.idVendor, dd.idProduct,
               libusb_get_bus_number(list[i]), libusb_get_device_address(list[i]));
        printf("  %-14s %d.%02d\n", "usb version",
               dd.bcdUSB >> 8, dd.bcdUSB & 0xff);
        printf("  %-14s %d.%02d\n", "device version",
               dd.bcdDevice >> 8, dd.bcdDevice & 0xff);
        printf("  %-14s %d\n", "configs", dd.bNumConfigurations);

        libusb_device_handle *h = NULL;
        int open_rc = libusb_open(list[i], &h);
        if (open_rc != 0) {
            printf("  (could not open device: %s — string descriptors unavailable)\n",
                   libusb_error_name(open_rc));
        } else {
            print_string(h, dd.iManufacturer, "manufacturer");
            print_string(h, dd.iProduct, "product");
            print_string(h, dd.iSerialNumber, "serial");
        }

        struct libusb_config_descriptor *cfg;
        if (libusb_get_active_config_descriptor(list[i], &cfg) == 0) {
            printf("\n  interfaces: %d\n", cfg->bNumInterfaces);
            for (int ii = 0; ii < cfg->bNumInterfaces; ii++) {
                const struct libusb_interface *itf = &cfg->interface[ii];
                for (int alt = 0; alt < itf->num_altsetting; alt++) {
                    const struct libusb_interface_descriptor *id = &itf->altsetting[alt];
                    // Only the default alt setting matters for a first survey.
                    if (id->bAlternateSetting != 0 && id->bNumEndpoints == 0) continue;
                    printf("\n    interface %d (alt %d): class 0x%02x/%02x (%s), %d endpoint(s)\n",
                           id->bInterfaceNumber, id->bAlternateSetting,
                           id->bInterfaceClass, id->bInterfaceSubClass,
                           class_name(id->bInterfaceClass), id->bNumEndpoints);
                    for (int e = 0; e < id->bNumEndpoints; e++) {
                        const struct libusb_endpoint_descriptor *ep = &id->endpoint[e];
                        printf("      ep 0x%02x  %-11s %-3s  max packet %d\n",
                               ep->bEndpointAddress,
                               xfer_name(ep->bmAttributes),
                               (ep->bEndpointAddress & 0x80) ? "IN" : "OUT",
                               ep->wMaxPacketSize);
                    }
                }
            }
            libusb_free_config_descriptor(cfg);
        }

        if (h && claim_iface >= 0) {
            printf("\n  --- claim test on interface %d ---\n", claim_iface);
            int cr = libusb_claim_interface(h, claim_iface);
            if (cr == 0) {
                printf("  CLAIMED interface %d successfully (no other owner)\n", claim_iface);
                libusb_release_interface(h, claim_iface);
            } else {
                printf("  claim FAILED: %s (%s)\n",
                       libusb_error_name(cr), libusb_strerror(cr));
                printf("  -> something else holds this interface (kernel driver or a running app)\n");
            }
        }

        if (h) libusb_close(h);
        printf("\n");
    }

    if (!found) printf("no device found for vid 0x%04x\n", want_vid);

    libusb_free_device_list(list, 1);
    libusb_exit(ctx);
    return found ? 0 : 2;
}
