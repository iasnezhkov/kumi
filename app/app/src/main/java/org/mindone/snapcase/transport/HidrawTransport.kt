// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import android.os.SystemClock
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import java.io.FileDescriptor
import org.mindone.snapcase.protocol.CaseException
import org.mindone.snapcase.protocol.CaseTimeoutException
import org.mindone.snapcase.protocol.ProtocolException
import org.mindone.snapcase.protocol.SnapProtocol
import org.mindone.snapcase.protocol.Transport

class HidrawTransport private constructor(private val path: String, private var fd: FileDescriptor?) : Transport {
    override val description = "hidraw $path"
    override val isDemo = false

    @Synchronized
    override fun exchange(request: ByteArray, timeoutMs: Long): ByteArray {
        if (request.size != SnapProtocol.REPORT_SIZE) throw ProtocolException("Expected a 64-byte request")
        val descriptor = fd ?: throw CaseException("$path is closed")
        drain(descriptor)
        try {
            val written = Os.write(descriptor, request, 0, request.size)
            if (written != request.size) throw CaseException("Short write to $path: $written")
        } catch (error: ErrnoException) {
            throw CaseException("Write to $path failed: ${error.message}", error)
        }
        val deadline = SystemClock.uptimeMillis() + timeoutMs
        val buffer = ByteArray(256)
        while (true) {
            val remaining = deadline - SystemClock.uptimeMillis()
            if (remaining <= 0) break
            val poll = StructPollfd().apply {
                this.fd = descriptor
                events = OsConstants.POLLIN.toShort()
            }
            val ready = try {
                Os.poll(arrayOf(poll), remaining.toInt())
            } catch (error: ErrnoException) {
                if (error.errno == OsConstants.EINTR) continue
                throw CaseException("poll on $path failed: ${error.message}", error)
            }
            if (ready <= 0) break
            val revents = poll.revents.toInt()
            if (revents and (OsConstants.POLLERR or OsConstants.POLLHUP or OsConstants.POLLNVAL) != 0) {
                throw CaseException("$path reported an error or hang-up")
            }
            val count = try {
                Os.read(descriptor, buffer, 0, buffer.size)
            } catch (error: ErrnoException) {
                if (error.errno == OsConstants.EAGAIN || error.errno == OsConstants.EINTR) continue
                throw CaseException("Read from $path failed: ${error.message}", error)
            }
            if (count >= 4 && SnapProtocol.matches(request, buffer)) return buffer.copyOf(count)
        }
        throw CaseTimeoutException("No matching reply within $timeoutMs ms")
    }

    @Synchronized
    override fun sendOnly(request: ByteArray) {
        if (request.size != SnapProtocol.REPORT_SIZE) throw ProtocolException("Expected a 64-byte request")
        val descriptor = fd ?: throw CaseException("$path is closed")
        drain(descriptor)
        try {
            val written = Os.write(descriptor, request, 0, request.size)
            if (written != request.size) throw CaseException("Short write to $path: $written")
        } catch (error: ErrnoException) {
            throw CaseException("Write to $path failed: ${error.message}", error)
        }
    }

    private fun drain(descriptor: FileDescriptor) {
        val scratch = ByteArray(256)
        repeat(MAX_DRAIN) {
            val count = try {
                Os.read(descriptor, scratch, 0, scratch.size)
            } catch (ignored: ErrnoException) {
                return
            }
            if (count <= 0) return
        }
    }

    @Synchronized
    override fun close() {
        fd?.let { runCatching { Os.close(it) } }
        fd = null
    }

    companion object {
        private const val MAX_DRAIN = 64

        fun open(path: String): HidrawTransport {
            if (!Regex("/dev/hidraw\\d+").matches(path)) throw IllegalArgumentException("Not a hidraw node: $path")
            val descriptor = try {
                Os.open(path, OsConstants.O_RDWR or OsConstants.O_NONBLOCK or OsConstants.O_CLOEXEC, 0)
            } catch (error: ErrnoException) {
                throw CaseException("Cannot open $path: ${error.message}", error)
            }
            return HidrawTransport(path, descriptor)
        }
    }
}
