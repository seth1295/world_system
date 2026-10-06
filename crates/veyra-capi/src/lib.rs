#![allow(unsafe_code)]

//! C ABI facade boundary. Stable C functions are deferred until the core call surface settles.

/// Returns the current C API contract version.
#[unsafe(no_mangle)]
pub extern "C" fn veyra_capi_abi_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_stable_abi_version() {
        assert_eq!(super::veyra_capi_abi_version(), 1);
    }
}
