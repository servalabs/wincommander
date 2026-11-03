// SPDX-License-Identifier: AGPL-3.0-or-later
//! Credential sealing only. The caller must independently verify the candidate
//! through the service-owned authenticated presence request before persistence.
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const MAX_SEALED: usize = 64 * 1024;
const INVALID: &str = "invalid_fleet_credential";
const UNAVAILABLE: &str = "fleet_credential_unavailable";

pub(crate) struct CredentialBinding<'a> {
    pub org_id: &'a str,
    pub device_id: &'a str,
    pub enrollment_generation: &'a str,
    /// The exact already-authorized origin; this layer does not normalize it.
    pub origin: &'a str,
}

impl CredentialBinding<'_> {
    fn entropy(&self) -> Result<[u8; 32], &'static str> {
        if [
            self.org_id,
            self.device_id,
            self.enrollment_generation,
            self.origin,
        ]
        .iter()
        .any(|field| field.is_empty())
        {
            return Err(INVALID);
        }
        let encoded = serde_json::to_vec(&[
            "wincommander:service-fleet-credential:v1",
            self.org_id,
            self.device_id,
            self.enrollment_generation,
            self.origin,
        ])
        .map_err(|_| INVALID)?;
        Ok(Sha256::digest(encoded).into())
    }
}

pub(crate) trait Protector {
    fn protect(&self, plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, &'static str>;
    fn unprotect(&self, sealed: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str>;
}

pub(crate) fn seal(
    protector: &dyn Protector,
    binding: &CredentialBinding<'_>,
    secret: &[u8],
) -> Result<Vec<u8>, &'static str> {
    if secret.len() != 32 {
        return Err(INVALID);
    }
    let sealed = protector.protect(secret, &binding.entropy()?)?;
    if sealed.is_empty() || sealed.len() > MAX_SEALED {
        return Err(UNAVAILABLE);
    }
    Ok(sealed)
}

pub(crate) fn unseal(
    protector: &dyn Protector,
    binding: &CredentialBinding<'_>,
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if sealed.is_empty() || sealed.len() > MAX_SEALED {
        return Err(INVALID);
    }
    let secret = protector.unprotect(sealed, &binding.entropy()?)?;
    if secret.len() != 32 {
        return Err(INVALID);
    }
    Ok(secret)
}

/// Machine DPAPI is an at-rest layer, not a service-only access boundary. The
/// caller must keep ciphertext in its existing protected service-owned store.
#[cfg(windows)]
pub(crate) struct MachineDpapi;

#[cfg(windows)]
impl Protector for MachineDpapi {
    fn protect(&self, plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, &'static str> {
        Ok(dpapi(plaintext, entropy, true)?.to_vec())
    }
    fn unprotect(&self, sealed: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        dpapi(sealed, entropy, false)
    }
}

#[cfg(windows)]
fn dpapi(input: &[u8], entropy: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE,
            CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    use zeroize::Zeroize;
    struct NativeOutput(CRYPT_INTEGER_BLOB);
    impl Drop for NativeOutput {
        fn drop(&mut self) {
            // DPAPI owns this successful output until LocalFree; wipe plaintext
            // before releasing its native allocation, including on early return.
            unsafe {
                if !self.0.pbData.is_null() {
                    std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                    LocalFree(self.0.pbData.cast());
                }
            }
        }
    }
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len().try_into().map_err(|_| INVALID)?,
        pbData: input.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len().try_into().map_err(|_| INVALID)?,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // Input/entropy slices remain alive for the synchronous API call; these APIs
    // read their blobs and return a separate LocalAlloc-owned output allocation.
    let success = unsafe {
        if protect {
            CryptProtectData(
                &source,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(UNAVAILABLE);
    }
    let output = NativeOutput(output);
    if output.0.pbData.is_null() || output.0.cbData == 0 || output.0.cbData as usize > MAX_SEALED {
        return Err(UNAVAILABLE);
    }
    // The successful API result supplies the allocation length; NativeOutput
    // retains it through the copy and wipes it before LocalFree on every exit.
    Ok(Zeroizing::new(unsafe {
        std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize).to_vec()
    }))
}

#[cfg(test)]
#[path = "fleet_credential_tests.rs"]
mod tests;
