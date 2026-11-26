// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#include <errno.h>
#include <fcntl.h>
#include <linux/hidraw.h>
#include <linux/usbdevice_fs.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

static long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000L + ts.tv_nsec / 1000000L;
}

static uint32_t le32(const uint8_t *p)
{
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

static uint32_t crc32(const uint8_t *p, size_t n)
{
    uint32_t crc = 0xffffffff;
    for (size_t i = 0; i < n; i++) {
        crc ^= p[i];
        for (int bit = 0; bit < 8; bit++)
            crc = (crc >> 1) ^ (0xedb88320U & (0U - (crc & 1)));
    }
    return ~crc;
}

#define SCRATCH_BASE 0x483f0000U
#define SCRATCH_END 0x48400000U

static int scratch_mode;
static uint32_t scratch_left;
static uint32_t write_base = SCRATCH_BASE;
static uint32_t program_at;
static int sector_erases_only;

static int ram_readable(uint32_t a, uint32_t n)
{
    if (!n)
        return 0;
    if (a < 0x200000 && n <= 0x200000 - a)
        return 1;
    return a >= 0x40000000U && a < 0x40040000U && n <= 0x40040000U - a;
}

static int ram_writable(uint32_t a, uint32_t n)
{
    if (!ram_readable(a, n))
        return 0;
    if (a < 0x102000 && a + n > 0x100000)
        return 0;
    if (a < 0x140000 && a + n > 0x138000)
        return 0;
    return 1;
}

static int valid_request(const uint8_t *p)
{
    if (memcmp(p, "BUXX", 4) || p[6] || le32(p + 10) != crc32(p, 10))
        return 0;
    unsigned command = p[4] | p[5] << 8;
    unsigned length = p[8] | p[9] << 8;
    unsigned end = 14;
    if (command == 0x110) {
        if (length || p[7] != 1)
            return 0;
    } else if (command == 0x123 || command == 0x124 || command == 0x125 ||
               command == 0x126 || command == 0x12b) {
        if (length || p[7])
            return 0;
    } else if (command == 0x122 || command == 0x128 || command == 0x12a) {
        if (length != 16 || p[7] || le32(p + 30) != crc32(p + 14, 16) ||
            le32(p + 22) != 256 || le32(p + 26))
            return 0;
        uint32_t address = le32(p + 14), size = le32(p + 18);
        scratch_left = 0;
        if (command == 0x122) {
            if (address < 0x48000000 || address >= 0x48400000 || !size ||
                size > 0x48400000 - address)
                return 0;
            if (scratch_mode && address >= write_base) {
                scratch_left = size;
                program_at = address;
            }
        } else if (command == 0x128) {
            if (!ram_writable(address, size))
                return 0;
        } else if (!ram_readable(address, size)) {
            return 0;
        }
        end = 34;
    } else if (command == 0x120 && scratch_mode) {
        uint32_t address = le32(p + 14), size = le32(p + 18);
        if (length != 16 || p[7] || le32(p + 30) != crc32(p + 14, 16) ||
            le32(p + 22) != 256 || le32(p + 26) || address < write_base ||
            address >= SCRATCH_END || (address & 0xfff) || !size || (size & 0xfff) ||
            size > SCRATCH_END - address || (sector_erases_only && size != 0x1000))
            return 0;
        scratch_left = 0;
        fprintf(stderr, "erase 0x%08x 0x%x\n", address, size);
        end = 34;
    } else if (command == 0x121 && scratch_mode) {
        if (!length || length > 256 || p[7] || le32(p + 14 + length) != crc32(p + 14, length) ||
            length > scratch_left)
            return 0;
        scratch_left -= length;
        fprintf(stderr, "program 0x%08x %u\n", program_at, length);
        program_at += length;
        end = 18 + length;
    } else if (command == 0x129) {
        if (!length || length > 256 || p[7] || le32(p + 14 + length) != crc32(p + 14, length))
            return 0;
        end = 18 + length;
    } else if (command == 0x12c) {
        if (length != 4 || p[7] || le32(p + 18) != crc32(p + 14, 4) || !ram_writable(le32(p + 14), 1))
            return 0;
        end = 22;
    } else {
        return 0;
    }
    for (unsigned i = end; i < 512; i++)
        if (p[i])
            return 0;
    return 1;
}

static int exact_io(int fd, void *buffer, size_t n, int writing)
{
    uint8_t *p = buffer;
    long deadline = now_ms() + 10000;
    while (n) {
        long remaining = deadline - now_ms();
        if (remaining <= 0)
            return -1;
        struct pollfd f = {.fd = fd, .events = writing ? POLLOUT : POLLIN};
        int ready = poll(&f, 1, (int)remaining);
        if (ready < 0 && errno == EINTR)
            continue;
        if (ready <= 0 || (f.revents & (POLLERR | POLLNVAL)))
            return -1;
        ssize_t rc = writing ? write(fd, p, n) : read(fd, p, n);
        if (rc < 0 && errno == EINTR)
            continue;
        if (rc <= 0)
            return -1;
        n -= (size_t)rc;
        p += rc;
    }
    return 0;
}

static int send_frame(uint8_t *data, unsigned n)
{
    uint8_t length[2] = {(uint8_t)n, (uint8_t)(n >> 8)};
    return exact_io(STDOUT_FILENO, length, 2, 1) || exact_io(STDOUT_FILENO, data, n, 1);
}

static int control(int fd, unsigned type, unsigned request, unsigned value, void *data, unsigned length)
{
    struct usbdevfs_ctrltransfer c = {.bRequestType = type, .bRequest = request,
        .wValue = value, .wLength = length, .timeout = 1000, .data = data};
    return ioctl(fd, USBDEVFS_CONTROL, &c);
}

static int transfer_within(int fd, uint8_t endpoint, uint8_t *buffer, unsigned length, long timeout_ms)
{
    struct usbdevfs_urb urb = {.type = USBDEVFS_URB_TYPE_INTERRUPT, .endpoint = endpoint,
        .buffer = buffer, .buffer_length = (int)length};
    if (ioctl(fd, USBDEVFS_SUBMITURB, &urb) < 0)
        return -1;
    long deadline = now_ms() + timeout_ms;
    for (;;) {
        void *done = NULL;
        if (ioctl(fd, USBDEVFS_REAPURBNDELAY, &done) == 0) {
            if (done != &urb || urb.status) {
                errno = urb.status < 0 ? -urb.status : EIO;
                return -1;
            }
            return urb.actual_length;
        }
        if (errno != EAGAIN && errno != EINTR)
            break;
        long remaining = deadline - now_ms();
        if (remaining <= 0) {
            errno = ETIMEDOUT;
            break;
        }
        struct pollfd f = {.fd = fd, .events = POLLOUT};
        poll(&f, 1, (int)remaining);
    }
    int saved = errno;
    ioctl(fd, USBDEVFS_DISCARDURB, &urb);
    void *done = NULL;
    ioctl(fd, USBDEVFS_REAPURB, &done);
    errno = saved;
    return -1;
}

static int transfer(int fd, uint8_t endpoint, uint8_t *buffer, unsigned length)
{
    return transfer_within(fd, endpoint, buffer, length, 1500);
}

static int enter(const char *path)
{
    int fd = open(path, O_RDWR | O_NONBLOCK | O_CLOEXEC);
    struct hidraw_devinfo info = {0};
    if (fd < 0 || ioctl(fd, HIDIOCGRAWINFO, &info) < 0 || info.bustype != 3 ||
        (uint16_t)info.vendor != 0x0661 || (uint16_t)info.product != 0x0881) {
        fprintf(stderr, "Expected Snap USB 0661:0881 HID\n");
        if (fd >= 0)
            close(fd);
        return 1;
    }
    const uint8_t request[64] = {0x4b, 0x01, 0xff};
    ssize_t written = write(fd, request, sizeof(request));
    int saved = errno;
    int rc = written == (ssize_t)sizeof(request) ? 0 : 1;
    if (!rc)
        fprintf(stderr, "Sent fixed 4b 01 ff runtime request\n");
    else if (written < 0 && saved == ETIMEDOUT)
        fprintf(stderr, "enter: the case did not accept the output report (timed out). The stock firmware stops "
                        "taking output reports after a long idle; detach and re-attach the case, then retry once. "
                        "Do not loop retries or toggle the USB role\n");
    else if (written < 0)
        fprintf(stderr, "enter: %s\n", strerror(saved));
    else
        fprintf(stderr, "enter: short write, %zd of %zu bytes\n", written, sizeof(request));
    close(fd);
    return rc;
}

static int bridge(const char *path)
{
    int fd = open(path, O_RDWR | O_CLOEXEC);
    if (fd < 0)
        return 1;
    uint8_t descriptors[4114], active = 0;
    if (control(fd, 0x80, 6, 0x100, descriptors, 18) != 18 ||
        descriptors[8] != 0x66 || descriptors[9] != 0x06 ||
        descriptors[10] != 0x88 || descriptors[11] != 0x08 || descriptors[17] != 1 ||
        control(fd, 0x80, 8, 0, &active, 1) != 1 ||
        control(fd, 0x80, 6, 0x200, descriptors + 18, 9) != 9)
        goto invalid;
    uint8_t *config = descriptors + 18;
    unsigned total = config[2] | config[3] << 8;
    if (total < 9 || total > 4096 || config[5] != active ||
        control(fd, 0x80, 6, 0x200, config, total) != (int)total)
        goto invalid;
    int selected = 0, found_interface = 0, endpoints = 0;
    for (unsigned i = 0; i + 2 <= total;) {
        unsigned n = config[i], type = config[i + 1];
        if (n < 2 || i + n > total)
            goto invalid;
        if (type == 4 && n >= 9) {
            selected = config[i + 2] == 0 && config[i + 3] == 0 && config[i + 5] == 3;
            if (selected) {
                if (found_interface)
                    goto invalid;
                found_interface = 1;
            }
        } else if (type == 5 && n >= 7 && selected) {
            unsigned size = (config[i + 4] | config[i + 5] << 8) & 0x7ff;
            if ((config[i + 3] & 3) != 3 || !size || size > 1024)
                goto invalid;
            if (config[i + 2] == 0x81)
                endpoints |= 1;
            else if (config[i + 2] == 0x02)
                endpoints |= 2;
        }
        i += n;
    }
    if (!found_interface || endpoints != 3)
        goto invalid;
    unsigned interface = 0;
    if (ioctl(fd, USBDEVFS_CLAIMINTERFACE, &interface) < 0) {
        struct usbdevfs_disconnect_claim claim = {.interface = 0};
        if (errno != EBUSY || ioctl(fd, USBDEVFS_DISCONNECT_CLAIM, &claim) < 0)
            goto invalid;
    }
    int result = 1;
    if (send_frame(descriptors, 18 + total))
        goto done;
    for (unsigned count = 0; count < 5000000; count++) {
        uint8_t request[512], reply[512];
        if (exact_io(STDIN_FILENO, request, sizeof(request), 0)) {
            result = 0;
            break;
        }
        if (!valid_request(request)) {
            fprintf(stderr, "Rejected command outside the read/return allowlist\n");
            break;
        }
        if (transfer(fd, 0x02, request, sizeof(request)) != sizeof(request)) {
            perror("USB OUT");
            break;
        }
        unsigned command = request[4] | request[5] << 8;
        long started = now_ms();
        int n = transfer_within(fd, 0x81, reply, sizeof(reply), command == 0x120 ? 20000 : 1500);
        if (command == 0x120 || command == 0x121)
            fprintf(stderr, "command=0x%03x reply_ms=%ld\n", command, now_ms() - started);
        if (n < 0) {
            perror("USB IN");
            break;
        }
        if (send_frame(reply, (unsigned)n))
            break;
    }
done:
    ioctl(fd, USBDEVFS_RELEASEINTERFACE, &interface);
    close(fd);
    return result;
invalid:
    fprintf(stderr, "Expected active BROM 0666:0888, HID interface 0, interrupt endpoints 81/02\n");
    close(fd);
    return 1;
}

#define KUMI_VID 0x1209
#define KUMI_PID 0x0001
#define KUMI_FRAME_LIMIT 4096
#define KUMI_OUT_TIMEOUT_MS 3000

struct kumi_link {
    int fd;
    unsigned interface, in_ep, out_ep, in_max, out_max;
};

static int kumi_find(const uint8_t *config, unsigned total, struct kumi_link *link)
{
    int selected = 0, found = 0;
    link->in_ep = link->out_ep = 0;
    for (unsigned i = 0; i + 2 <= total;) {
        unsigned n = config[i], type = config[i + 1];
        if (n < 2 || i + n > total)
            return -1;
        if (type == 4 && n >= 9) {
            selected = config[i + 3] == 0 && config[i + 5] == 0xff && config[i + 6] == 0x4b && config[i + 7] == 0x01;
            if (selected) {
                if (found)
                    return -1;
                found = 1;
                link->interface = config[i + 2];
            }
        } else if (type == 5 && n >= 7 && selected) {
            unsigned address = config[i + 2], size = (config[i + 4] | config[i + 5] << 8) & 0x7ff;
            if ((config[i + 3] & 3) != 2 || !size)
                return -1;
            if (address & 0x80) {
                link->in_ep = address;
                link->in_max = size;
            } else {
                link->out_ep = address;
                link->out_max = size;
            }
        }
        i += n;
    }
    return found && link->in_ep && link->out_ep ? 0 : -1;
}

static int kumi_submit(int fd, struct usbdevfs_urb *urb, unsigned endpoint, uint8_t *buffer, unsigned length)
{
    memset(urb, 0, sizeof(*urb));
    urb->type = USBDEVFS_URB_TYPE_BULK;
    urb->endpoint = (unsigned char)endpoint;
    urb->buffer = buffer;
    urb->buffer_length = (int)length;
    if (ioctl(fd, USBDEVFS_SUBMITURB, urb) < 0) {
        perror("USB submit");
        return -1;
    }
    return 0;
}

static void kumi_cancel(int fd, struct usbdevfs_urb *urb)
{
    if (ioctl(fd, USBDEVFS_DISCARDURB, urb) == 0) {
        void *done = NULL;
        ioctl(fd, USBDEVFS_REAPURB, &done);
    }
}

static int kumi_read_frame(uint8_t *buffer, unsigned *length, int *eof)
{
    uint8_t prefix[2];
    ssize_t rc = read(STDIN_FILENO, prefix, 1);
    if (rc == 0) {
        *eof = 1;
        return 0;
    }
    if (rc < 0)
        return errno == EINTR || errno == EAGAIN ? 0 : -1;
    if (exact_io(STDIN_FILENO, prefix + 1, 1, 0))
        return -1;
    *length = prefix[0] | prefix[1] << 8;
    if (!*length || *length > KUMI_FRAME_LIMIT || exact_io(STDIN_FILENO, buffer, *length, 0)) {
        fprintf(stderr, "Host frame must hold 1..%u bytes\n", KUMI_FRAME_LIMIT);
        return -1;
    }
    return 1;
}

static int kumi_loop(struct kumi_link *link)
{
    static uint8_t in_buffer[KUMI_FRAME_LIMIT], frame[KUMI_FRAME_LIMIT], out_buffer[KUMI_FRAME_LIMIT];
    struct usbdevfs_urb in, out;
    unsigned frame_length = 0, out_length = 0;
    int out_busy = 0, zero_pending = 0, eof = 0, result = 1;
    long out_started = 0;
    if (kumi_submit(link->fd, &in, link->in_ep, in_buffer, sizeof(in_buffer)))
        return 1;
    for (;;) {
        struct pollfd f[2] = {{.fd = link->fd, .events = POLLOUT}, {.fd = STDIN_FILENO, .events = POLLIN}};
        int count = !eof && !out_busy ? 2 : 1;
        if (poll(f, count, 50) < 0 && errno != EINTR) {
            perror("poll");
            goto done;
        }
        if (f[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            fprintf(stderr, "USB device gone\n");
            goto done;
        }
        for (;;) {
            struct usbdevfs_urb *completed = NULL;
            if (ioctl(link->fd, USBDEVFS_REAPURBNDELAY, &completed) < 0) {
                if (errno == EAGAIN)
                    break;
                perror("USB reap");
                goto done;
            }
            if (completed == &in) {
                unsigned n = (unsigned)in.actual_length;
                if (in.status) {
                    errno = in.status < 0 ? -in.status : EIO;
                    perror("USB IN");
                    goto done;
                }
                if (frame_length + n > sizeof(frame)) {
                    fprintf(stderr, "Device frame exceeds %u bytes\n", KUMI_FRAME_LIMIT);
                    goto done;
                }
                memcpy(frame + frame_length, in_buffer, n);
                frame_length += n;
                if (n < sizeof(in_buffer)) {
                    if (frame_length && send_frame(frame, frame_length))
                        goto done;
                    frame_length = 0;
                }
                if (kumi_submit(link->fd, &in, link->in_ep, in_buffer, sizeof(in_buffer)))
                    goto done;
            } else if (completed == &out) {
                if (out.status || out.actual_length != out.buffer_length) {
                    fprintf(stderr, "USB OUT failed: status %d, %d of %d bytes\n", out.status, out.actual_length,
                            out.buffer_length);
                    out_busy = 0;
                    goto done;
                }
                if (zero_pending) {
                    zero_pending = 0;
                    if (kumi_submit(link->fd, &out, link->out_ep, out_buffer, 0)) {
                        out_busy = 0;
                        goto done;
                    }
                    out_started = now_ms();
                } else {
                    out_busy = 0;
                }
            }
        }
        if (out_busy && now_ms() - out_started > KUMI_OUT_TIMEOUT_MS) {
            fprintf(stderr, "USB OUT timed out after %d ms\n", KUMI_OUT_TIMEOUT_MS);
            goto done;
        }
        if (eof && !out_busy) {
            result = 0;
            goto done;
        }
        if (count == 2 && (f[1].revents & (POLLIN | POLLHUP))) {
            int rc = kumi_read_frame(out_buffer, &out_length, &eof);
            if (rc < 0)
                goto done;
            if (rc > 0) {
                zero_pending = out_length % link->out_max == 0;
                if (kumi_submit(link->fd, &out, link->out_ep, out_buffer, out_length))
                    goto done;
                out_busy = 1;
                out_started = now_ms();
            }
        }
    }
done:
    if (out_busy)
        kumi_cancel(link->fd, &out);
    kumi_cancel(link->fd, &in);
    return result;
}

static int kumi_bridge(const char *path)
{
    int fd = open(path, O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        perror("open");
        return 1;
    }
    static uint8_t descriptors[18 + 4096];
    uint8_t active = 0;
    struct kumi_link link = {.fd = fd};
    if (control(fd, 0x80, 6, 0x100, descriptors, 18) != 18 || descriptors[1] != 1 ||
        (descriptors[8] | descriptors[9] << 8) != KUMI_VID || (descriptors[10] | descriptors[11] << 8) != KUMI_PID ||
        control(fd, 0x80, 8, 0, &active, 1) != 1 || !active ||
        control(fd, 0x80, 6, 0x200, descriptors + 18, 9) != 9)
        goto invalid;
    uint8_t *config = descriptors + 18;
    unsigned total = config[2] | config[3] << 8;
    if (total < 9 || total > 4096 || config[5] != active ||
        control(fd, 0x80, 6, 0x200, config, total) != (int)total || kumi_find(config, total, &link))
        goto invalid;
    if (ioctl(fd, USBDEVFS_CLAIMINTERFACE, &link.interface) < 0) {
        struct usbdevfs_disconnect_claim claim = {.interface = link.interface};
        if (errno != EBUSY || ioctl(fd, USBDEVFS_DISCONNECT_CLAIM, &claim) < 0) {
            perror("claim");
            close(fd);
            return 1;
        }
    }
    fprintf(stderr, "Kumi interface %u: IN 0x%02x max %u, OUT 0x%02x max %u\n", link.interface, link.in_ep,
            link.in_max, link.out_ep, link.out_max);
    int result = send_frame(descriptors, 18 + total) ? 1 : kumi_loop(&link);
    ioctl(fd, USBDEVFS_RELEASEINTERFACE, &link.interface);
    close(fd);
    return result;
invalid:
    fprintf(stderr, "Expected configured Kumi 1209:0001 with one vendor interface FF/4B/01 and bulk IN and OUT\n");
    close(fd);
    return 1;
}

int main(int argc, char **argv)
{
    if (argc == 3 && !strcmp(argv[1], "enter"))
        return enter(argv[2]);
    if (argc == 3 && !strcmp(argv[1], "bridge"))
        return bridge(argv[2]);
    if (argc == 3 && !strcmp(argv[1], "kumi-bridge"))
        return kumi_bridge(argv[2]);
    if (argc == 3 && !strcmp(argv[1], "scratch-bridge")) {
        scratch_mode = 1;
        return bridge(argv[2]);
    }
    if (argc == 3 && !strcmp(argv[1], "install-bridge")) {
        scratch_mode = 1;
        write_base = 0x48000000U;
        sector_erases_only = 1;
        return bridge(argv[2]);
    }
    if ((argc == 2 || argc == 3) && !strcmp(argv[1], "validate")) {
        if (argc == 3 && !strcmp(argv[2], "scratch")) {
            scratch_mode = 1;
        } else if (argc == 3 && !strcmp(argv[2], "install")) {
            scratch_mode = 1;
            write_base = 0x48000000U;
            sector_erases_only = 1;
        } else if (argc == 3) {
            return 2;
        }
        uint8_t request[512];
        return exact_io(STDIN_FILENO, request, sizeof(request), 0) || !valid_request(request);
    }
    fprintf(stderr, "usage: spv40-usb enter /dev/hidrawN | bridge /dev/bus/usb/BBB/DDD | scratch-bridge /dev/bus/usb/BBB/DDD (erase/program only in flash 0x3F0000-0x3FFFFF) | install-bridge /dev/bus/usb/BBB/DDD (erase one 4 KiB sector or program anywhere in flash, every write logged; for kumi-cli brom install and restore) | kumi-bridge /dev/bus/usb/BBB/DDD (Kumi 1209:0001 frames as u16 length + bytes on stdin/stdout) | validate [scratch|install] (one 512-byte request on stdin, exit 0 when allowed)\n");
    return 2;
}
