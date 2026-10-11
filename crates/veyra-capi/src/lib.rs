#![allow(unsafe_code)]

//! C ABI facade boundary. Stable C functions are deferred until the core call surface settles.

/// Returns the current C API contract version.
#[unsafe(no_mangle)]
pub extern "C" fn veyra_capi_abi_version() -> u32 {
    1
}

#[cfg(feature = "foundation-parity")]
mod foundation_parity;

#[cfg(feature = "foundation-parity")]
pub use foundation_parity::{
    foundation_parity_expected_hash, foundation_parity_payload, foundation_parity_sample_count,
};

/// Test-only binary payload export used by the native/WASM foundation parity harness.
#[cfg(all(feature = "foundation-parity", target_arch = "wasm32"))]
#[unsafe(no_mangle)]
pub extern "C" fn veyra_capi_foundation_parity_payload() -> u64 {
    let Ok(payload) = foundation_parity_payload() else { return 0 };
    let Ok(length) = u32::try_from(payload.len()) else { return 0 };
    let payload = Box::leak(payload.into_boxed_slice());
    let Ok(pointer) = u32::try_from(payload.as_ptr() as usize) else { return 0 };
    (u64::from(length) << 32) | u64::from(pointer)
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_stable_abi_version() {
        assert_eq!(super::veyra_capi_abi_version(), 1);
    }
}
