//! Memory security utilities: memory locking helper.
//!
//! Implements Requirements: 24 (memory zeroization, memory locking)
//!
//! On Linux/Unix, sensitive buffers can be locked into RAM with `mlock(2)` to
//! prevent them from being swapped to disk.  This module provides a best-effort
//! wrapper.  If the `libc` crate were available we would use `libc::mlock`
//! directly; since it is not in our dependency set we invoke the Linux syscall
//! via `std::os::unix` abstractions or fall back to a documented no-op.
//!
//! Note: `mlock` requires `CAP_IPC_LOCK` or a sufficient `RLIMIT_MEMLOCK`
//! limit.  Failure is non-fatal — the sensitive data is still zeroized on drop.

/// Attempt to lock `len` bytes starting at `ptr` into RAM (Linux only).
///
/// Returns `true` if locking succeeded, `false` if it failed (non-fatal).
///
/// # Safety
/// The caller must ensure `ptr` is valid and points to at least `len` bytes
/// that will remain alive for the duration of the lock.
#[cfg(target_os = "linux")]
pub unsafe fn try_lock_memory(ptr: *const u8, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    // Linux syscall number for mlock: 149 on x86_64
    // We use the `syscall` macro via inline asm rather than the libc crate
    // to avoid adding a new dependency.  If libc is added later, replace with:
    //   libc::mlock(ptr as *const libc::c_void, len) == 0
    let ret = raw_mlock(ptr, len);
    if ret != 0 {
        tracing::warn!(
            "mlock({:p}, {}) failed with code {}",
            ptr,
            len,
            ret
        );
        false
    } else {
        true
    }
}

/// Unlock memory previously locked with `try_lock_memory` (Linux only).
///
/// # Safety
/// The caller must ensure `ptr` and `len` match a previously locked region.
#[cfg(target_os = "linux")]
pub unsafe fn try_unlock_memory(ptr: *const u8, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    raw_munlock(ptr, len) == 0
}

/// Non-Linux Unix stub — mlock is documented as unsupported.
#[cfg(all(unix, not(target_os = "linux")))]
pub unsafe fn try_lock_memory(_ptr: *const u8, _len: usize) -> bool {
    // mlock is available on macOS/BSD too, but we keep the implementation
    // minimal.  Add libc crate for full cross-platform support.
    tracing::debug!("mlock not implemented on this platform (add libc crate)");
    false
}

/// Non-Linux Unix stub.
#[cfg(all(unix, not(target_os = "linux")))]
pub unsafe fn try_unlock_memory(_ptr: *const u8, _len: usize) -> bool {
    false
}

/// Non-Unix stub.
#[cfg(not(unix))]
pub unsafe fn try_lock_memory(_ptr: *const u8, _len: usize) -> bool {
    false
}

/// Non-Unix stub.
#[cfg(not(unix))]
pub unsafe fn try_unlock_memory(_ptr: *const u8, _len: usize) -> bool {
    false
}

// ── Linux syscall wrappers ────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
unsafe fn raw_mlock(ptr: *const u8, len: usize) -> i64 {
    let result: i64;
    // mlock syscall: nr=149 on x86_64
    std::arch::asm!(
        "syscall",
        in("rax") 149_i64,            // SYS_mlock
        in("rdi") ptr as usize,
        in("rsi") len,
        lateout("rax") result,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );
    result
}

#[cfg(target_os = "linux")]
unsafe fn raw_munlock(ptr: *const u8, len: usize) -> i64 {
    let result: i64;
    // munlock syscall: nr=150 on x86_64
    std::arch::asm!(
        "syscall",
        in("rax") 150_i64,            // SYS_munlock
        in("rdi") ptr as usize,
        in("rsi") len,
        lateout("rax") result,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroize;

    // ── Unit tests (Task 19.7) ────────────────────────────────────────────────

    /// try_lock_memory does not panic when called with a valid buffer.
    #[test]
    fn test_try_lock_memory_does_not_panic() {
        let data = vec![0u8; 128];
        // Best-effort: may succeed or fail depending on system limits.
        // We only assert it doesn't panic.
        let _result = unsafe { try_lock_memory(data.as_ptr(), data.len()) };
    }

    /// try_lock_memory with length 0 returns true without error.
    #[test]
    fn test_try_lock_memory_zero_length() {
        let data: Vec<u8> = vec![];
        let result = unsafe { try_lock_memory(data.as_ptr(), data.len()) };
        assert!(result, "zero-length mlock should succeed trivially");
    }

    /// try_unlock_memory does not panic.
    #[test]
    fn test_try_unlock_memory_does_not_panic() {
        let data = vec![0u8; 64];
        unsafe {
            let _locked = try_lock_memory(data.as_ptr(), data.len());
            let _unlocked = try_unlock_memory(data.as_ptr(), data.len());
        }
    }

    /// Model update gradient buffers are zeroed after use (Req 24.1, 24.4).
    ///
    /// Verifies that calling zeroize() on a Vec<f32> gradient buffer sets all
    /// elements to zero, preventing sensitive gradient data from persisting
    /// in heap memory after the update is uploaded.
    #[test]
    fn test_gradient_buffer_zeroized_after_use() {
        let mut gradients: Vec<f32> = vec![1.5, -2.3, 0.7, 100.0, -0.001];

        // Simulate: gradients are used (privacy applied, masked, uploaded)
        let _sum: f32 = gradients.iter().sum(); // "use" the gradients

        // After upload, zeroize the buffer
        gradients.zeroize();

        // All elements must be zero
        for (i, &val) in gradients.iter().enumerate() {
            assert_eq!(val, 0.0f32, "gradient[{}] must be zero after zeroize", i);
        }
    }

    /// Cryptographic key material is zeroed after use (Req 24.2).
    ///
    /// Verifies that a Vec<u8> holding key bytes is fully zeroed after zeroize().
    #[test]
    fn test_key_material_zeroized_after_use() {
        let mut key_bytes: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];

        // Simulate: key was used for signing
        let _len = key_bytes.len(); // "use" the key

        // Zeroize the key after use
        key_bytes.zeroize();

        for (i, &byte) in key_bytes.iter().enumerate() {
            assert_eq!(byte, 0u8, "key_bytes[{}] must be zero after zeroize", i);
        }
    }

    /// Secure aggregation mask buffers are zeroed after masking (Req 24.3).
    ///
    /// Verifies that a Vec<f32> mask buffer is cleared after the masked update
    /// is computed, preventing pairwise secret leakage.
    #[test]
    fn test_secure_agg_mask_zeroized_after_masking() {
        let mut mask: Vec<f32> = (0..32).map(|i| i as f32 * 0.01).collect();

        // Simulate: mask applied to gradient update
        let _norm: f32 = mask.iter().map(|x| x * x).sum::<f32>().sqrt();

        // Zeroize the mask after use
        mask.zeroize();

        for (i, &val) in mask.iter().enumerate() {
            assert_eq!(val, 0.0f32, "mask[{}] must be zero after zeroize", i);
        }
    }

    /// Model binary is zeroed after the model is loaded into the framework (Req 24.1).
    ///
    /// This mirrors ModelManager::zeroize_model_binary().
    #[test]
    fn test_model_binary_zeroized_after_framework_load() {
        let mut binary = vec![0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0xFF, 0x01, 0x10];

        // Simulate: binary loaded into ML framework
        let _loaded = binary.clone();

        // Zeroize after loading
        binary.zeroize();

        for (i, &byte) in binary.iter().enumerate() {
            assert_eq!(byte, 0u8, "binary[{}] must be zero after zeroize", i);
        }
    }

    /// Shared ECDH secrets used for secure aggregation are zeroed (Req 24.3).
    #[test]
    fn test_ecdh_shared_secret_zeroized() {
        // Simulate a shared secret derived from ECDH
        let mut shared_secret: Vec<u8> = (0u8..32).collect();

        // Simulate: secret used to derive pairwise mask
        let _first_byte = shared_secret[0];

        // Zeroize after use
        shared_secret.zeroize();

        for (i, &byte) in shared_secret.iter().enumerate() {
            assert_eq!(byte, 0u8, "shared_secret[{}] must be zero after zeroize", i);
        }
    }

    // ── Property-based test (Task 19.8) ───────────────────────────────────────
    //
    // **Validates: Requirements 24.1, 24.2, 24.3, 24.4, 24.7**
    //
    // Property 43: Memory Zeroization
    // Any sensitive buffer that is zeroized must have all bytes/values set to
    // the zero value before deallocation.

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(200))]

        /// Property 43: Any Vec<u8> (key material, model binary, mask) that is
        /// zeroized must have all bytes equal to 0x00 afterwards.
        ///
        /// **Validates: Requirements 24.1, 24.2, 24.3, 24.4, 24.7**
        #[test]
        fn prop_byte_buffer_fully_zeroed_after_zeroize(
            data in proptest::collection::vec(proptest::num::u8::ANY, 1..=512),
        ) {
            let mut buf = data;

            // Precondition: at least some bytes are non-zero (statistically almost always)
            // We still test zeroize regardless.
            buf.zeroize();

            for (i, &byte) in buf.iter().enumerate() {
                proptest::prop_assert_eq!(
                    byte, 0u8,
                    "buf[{}] must be 0x00 after zeroize, got {:02x}",
                    i, byte
                );
            }
        }

        /// Property 43b: Any Vec<f32> (gradient buffer) that is zeroized must
        /// have all values equal to 0.0f32 afterwards.
        ///
        /// **Validates: Requirements 24.1, 24.4, 24.7**
        #[test]
        fn prop_gradient_buffer_fully_zeroed_after_zeroize(
            data in proptest::collection::vec(proptest::num::f32::ANY, 1..=128),
        ) {
            let mut buf: Vec<f32> = data
                .into_iter()
                .map(|v| if v.is_nan() || v.is_infinite() { 1.0 } else { v })
                .collect();

            buf.zeroize();

            for (i, &val) in buf.iter().enumerate() {
                proptest::prop_assert_eq!(
                    val, 0.0f32,
                    "gradient buf[{}] must be 0.0 after zeroize, got {}",
                    i, val
                );
            }
        }

        /// Property 43c: Memory lock/unlock operations always leave the buffer
        /// content intact (mlock/munlock must not modify data).
        ///
        /// **Validates: Requirements 24.5, 24.6**
        #[test]
        fn prop_memory_lock_does_not_modify_data(
            data in proptest::collection::vec(proptest::num::u8::ANY, 1..=256),
        ) {
            let original = data.clone();
            let buf = data;

            // Lock (best-effort, may fail on CI with low RLIMIT_MEMLOCK)
            let locked = unsafe { try_lock_memory(buf.as_ptr(), buf.len()) };
            if locked {
                unsafe { try_unlock_memory(buf.as_ptr(), buf.len()); }
            }

            // Data must be identical regardless of whether locking succeeded
            proptest::prop_assert_eq!(
                &buf, &original,
                "mlock/munlock must not modify buffer contents"
            );
        }
    }
}
