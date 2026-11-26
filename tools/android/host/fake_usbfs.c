// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#include <errno.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <linux/hidraw.h>
#include <linux/usbdevice_fs.h>

#define QUEUE 4096
#define FRAME 8192

static uint8_t in_queue[QUEUE][512];
static unsigned in_sizes[QUEUE], in_head, in_tail;
static uint8_t request[FRAME];
static unsigned request_length;
static struct usbdevfs_urb *pending_in, *completed[8];
static unsigned completed_count;
static int started;

static unsigned max_packet(void)
{
    const char *value = getenv("FAKE_USB_MAX");
    return value ? (unsigned)strtoul(value, NULL, 0) : 64;
}

static FILE *trace(void)
{
    static FILE *file;
    if (!file) {
        const char *path = getenv("FAKE_USB_LOG");
        file = path ? fopen(path, "w") : stderr;
        setvbuf(file, NULL, _IOLBF, 0);
    }
    return file;
}

static void queue_frame(const uint8_t *data, unsigned length)
{
    unsigned size = max_packet();
    for (unsigned at = 0; at < length; at += size) {
        unsigned n = length - at < size ? length - at : size;
        memcpy(in_queue[in_tail % QUEUE], data + at, n);
        in_sizes[in_tail++ % QUEUE] = n;
    }
    if (length % size == 0)
        in_sizes[in_tail++ % QUEUE] = 0;
}

static void start(void)
{
    if (started)
        return;
    started = 1;
    unsigned size = max_packet();
    static uint8_t event[512];
    for (unsigned i = 0; i < size; i++)
        event[i] = (uint8_t)(0xE0 + i);
    queue_frame(event, size);
}

static void device_packet(const uint8_t *data, unsigned n)
{
    fprintf(trace(), "OUT %u\n", n);
    if (request_length + n > FRAME)
        abort();
    memcpy(request + request_length, data, n);
    request_length += n;
    if (n < max_packet()) {
        for (unsigned i = 0; i < request_length; i++)
            request[i] ^= 0x5A;
        queue_frame(request, request_length);
        request_length = 0;
    }
}

static void complete(struct usbdevfs_urb *urb, int status)
{
    urb->status = status;
    completed[completed_count++] = urb;
}

static void fill_in(void)
{
    if (!pending_in)
        return;
    struct usbdevfs_urb *urb = pending_in;
    uint8_t *buffer = urb->buffer;
    while (in_head != in_tail) {
        unsigned n = in_sizes[in_head % QUEUE];
        if ((unsigned)urb->actual_length + n > (unsigned)urb->buffer_length)
            abort();
        memcpy(buffer + urb->actual_length, in_queue[in_head % QUEUE], n);
        urb->actual_length += (int)n;
        in_head++;
        fprintf(trace(), "IN %u\n", n);
        if (n < max_packet() || urb->actual_length == urb->buffer_length) {
            pending_in = NULL;
            complete(urb, 0);
            return;
        }
    }
}

static int control_transfer(struct usbdevfs_ctrltransfer *c)
{
    unsigned size = max_packet();
    const char *product = getenv("FAKE_USB_PID");
    uint16_t pid = product ? (uint16_t)strtoul(product, NULL, 0) : 1;
    uint8_t device[18] = {18, 1, 0, 2, 0, 0, 0, 64, 0x09, 0x12, (uint8_t)pid, (uint8_t)(pid >> 8), 0, 1, 1, 2, 0, 1};
    uint8_t config[41] = {9, 2, 41, 0, 2, 1, 0, 0x80, 50,
                          9, 4, 0, 0, 1, 3, 1, 1, 0,
                          7, 5, 0x81, 3, 16, 0, 1,
                          9, 4, 1, 0, 2, 0xff, 0x4b, 0x01, 0,
                          7, 5, 0x82, 2, (uint8_t)size, (uint8_t)(size >> 8), 0};
    uint8_t out_endpoint[7] = {7, 5, 0x02, 2, (uint8_t)size, (uint8_t)(size >> 8), 0};
    static uint8_t full[48];
    memcpy(full, config, sizeof(config));
    memcpy(full + sizeof(config), out_endpoint, sizeof(out_endpoint));
    full[2] = sizeof(full);
    if (c->bRequest == 6 && c->wValue == 0x100) {
        unsigned n = c->wLength < 18 ? c->wLength : 18;
        memcpy(c->data, device, n);
        return (int)n;
    }
    if (c->bRequest == 6 && c->wValue == 0x200) {
        unsigned n = c->wLength < sizeof(full) ? c->wLength : sizeof(full);
        memcpy(c->data, full, n);
        return (int)n;
    }
    if (c->bRequest == 8) {
        *(uint8_t *)c->data = 1;
        return 1;
    }
    errno = EPIPE;
    return -1;
}

int ioctl(int fd, unsigned long request_code, ...)
{
    va_list arguments;
    va_start(arguments, request_code);
    void *argument = va_arg(arguments, void *);
    va_end(arguments);
    (void)fd;
    start();
    if (request_code == USBDEVFS_CONTROL)
        return control_transfer(argument);
    if (request_code == USBDEVFS_CLAIMINTERFACE || request_code == USBDEVFS_RELEASEINTERFACE) {
        fprintf(trace(), "%s %u\n", request_code == USBDEVFS_CLAIMINTERFACE ? "CLAIM" : "RELEASE",
                *(unsigned *)argument);
        return 0;
    }
    if (request_code == USBDEVFS_SUBMITURB) {
        struct usbdevfs_urb *urb = argument;
        urb->actual_length = 0;
        if (urb->type != USBDEVFS_URB_TYPE_BULK) {
            errno = EINVAL;
            return -1;
        }
        if (urb->endpoint & 0x80) {
            if (urb->endpoint != 0x82 || pending_in) {
                errno = EINVAL;
                return -1;
            }
            pending_in = urb;
            fill_in();
            return 0;
        }
        if (urb->endpoint != 0x02) {
            errno = EINVAL;
            return -1;
        }
        unsigned size = max_packet(), length = (unsigned)urb->buffer_length;
        if (!length)
            device_packet(urb->buffer, 0);
        for (unsigned at = 0; at < length; at += size)
            device_packet((uint8_t *)urb->buffer + at, length - at < size ? length - at : size);
        urb->actual_length = urb->buffer_length;
        complete(urb, 0);
        fill_in();
        return 0;
    }
    if (request_code == USBDEVFS_DISCARDURB) {
        if (argument != pending_in) {
            errno = EINVAL;
            return -1;
        }
        pending_in = NULL;
        complete(argument, -ENOENT);
        return 0;
    }
    if (request_code == USBDEVFS_REAPURB || request_code == USBDEVFS_REAPURBNDELAY) {
        fill_in();
        if (!completed_count) {
            errno = EAGAIN;
            return -1;
        }
        *(void **)argument = completed[0];
        memmove(completed, completed + 1, --completed_count * sizeof(completed[0]));
        return 0;
    }
    errno = ENOTTY;
    return -1;
}
