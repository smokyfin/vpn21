package com.vpn21.app

/**
 * Thin Kotlin wrapper around the JNI symbols exported by `libvpn21.so`.
 *
 * The TUN file descriptor never crosses the Dart boundary.  The Kotlin
 * `Vpn21VpnService` opens the fd via `VpnService.Builder.establish()` and
 * hands it directly to Rust via [nativeStartWithFd]; Rust then runs leaf's
 * built-in `inbound-tun` against that fd, so all packet I/O stays inside
 * `vpn21-core`.
 *
 * The init / status / stop entrypoints are also exposed so the service
 * can drive the lifecycle end-to-end without bouncing through Dart.
 */
object Vpn21Native {
    init {
        System.loadLibrary("vpn21")
    }

    /** One-shot initialisation; safe to call from multiple processes. */
    @JvmStatic external fun nativeInit(appDir: String, verbose: Boolean): Int

    /**
     * Adopts the platform-provided TUN fd and starts the full pipeline
     * (leaf #1 + arti + leaf #2 + DNS server).
     *
     * Returns the JSON the C-ABI also returns: `{"ok":true,"detail":...}`
     * on success or `{"ok":false,"error":...}` on failure.
     */
    @JvmStatic external fun nativeStartWithFd(
        profileJson: String,
        fd: Int,
        mtu: Int,
        ipv4: String,
        mask: Int,
        dnsPort: Int,
    ): String

    /** Tears the pipeline down and releases all sockets / threads. */
    @JvmStatic external fun nativeStop(): String

    /** Returns the current `{state, detail, progress}` JSON. */
    @JvmStatic external fun nativeStatus(): String
}
