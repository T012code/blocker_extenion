//! Protects the saved sign-in with Windows' DPAPI: only this Windows account on this PC
//! can read it back.

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;

fn take(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    unsafe {
        let out = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(blob.pbData as *mut _)));
        out
    }
}

pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let name = super::wide("Smart Clip sign-in");
    unsafe {
        CryptProtectData(&input, PCWSTR(name.as_ptr()), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            .ok()?;
    }
    Some(take(output))
}

pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
    }
    Some(take(output))
}
