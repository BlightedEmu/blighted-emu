// SPDX-License-Identifier: AGPL-3.0-or-later
//! LZ4 vtable slots from protocol note section 2.
//!
//! The three wrappers have no direct call. This crate does not compress or decompress a datagram.
//! Factory `FUN_146ab5400` stores vtable `0x148588db8`.
//! `javelin.enable-lz4-compression` is used at `0x14618048a` and does not call the wrappers.

/// Vtable base stored by `FUN_146ab5400`.
pub const LZ4_VTABLE: u64 = 0x1485_88DB8;

/// Slot `+0x20`. `0x146abec70` calls `LZ4_compressBound`.
pub const LZ4_SLOT_COMPRESS_BOUND: u64 = 0x20;

/// Slot `+0x28`. Function `0x146ab3bd0`. The note does not name it.
pub const LZ4_SLOT_PLUS_0X28: u64 = 0x28;

/// Slot `+0x30`. Function `0x146ab6540`. The note does not name it.
pub const LZ4_SLOT_PLUS_0X30: u64 = 0x30;

/// Address of the `+0x20` slot.
pub const LZ4_COMPRESS_BOUND_SLOT_ADDRESS: u64 = 0x1485_88DD8;

/// Address of the `+0x28` slot.
pub const LZ4_PLUS_0X28_SLOT_ADDRESS: u64 = 0x1485_88DE0;

/// Address of the `+0x30` slot.
pub const LZ4_PLUS_0X30_SLOT_ADDRESS: u64 = 0x1485_88DE8;

/// `LZ4_compressBound` wrapper at the `+0x20` slot.
pub const LZ4_COMPRESS_BOUND_WRAPPER: u64 = 0x146A_BEC70;

/// Function stored at the `+0x28` slot.
pub const LZ4_FUNCTION_AT_PLUS_0X28: u64 = 0x146A_B3BD0;

/// Function stored at the `+0x30` slot.
pub const LZ4_FUNCTION_AT_PLUS_0X30: u64 = 0x146A_B6540;

/// `FUN_146ab5400`. It has no code caller.
pub const LZ4_FACTORY: u64 = 0x146A_B5400;

/// Use of `javelin.enable-lz4-compression` inside `FUN_14616c670`.
pub const LZ4_CONFIG_USE: u64 = 0x1461_8048A;

/// The three wrapper addresses have no call instruction.
pub const LZ4_WRAPPERS_HAVE_DIRECT_CALL: bool = false;

const _: () = assert!(!LZ4_WRAPPERS_HAVE_DIRECT_CALL);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrappers_have_no_direct_call_and_the_slots_match_the_note() {
        assert_eq!(
            LZ4_VTABLE + LZ4_SLOT_COMPRESS_BOUND,
            LZ4_COMPRESS_BOUND_SLOT_ADDRESS
        );
        assert_eq!(LZ4_VTABLE + LZ4_SLOT_PLUS_0X28, LZ4_PLUS_0X28_SLOT_ADDRESS);
        assert_eq!(LZ4_VTABLE + LZ4_SLOT_PLUS_0X30, LZ4_PLUS_0X30_SLOT_ADDRESS);
        assert_ne!(LZ4_COMPRESS_BOUND_WRAPPER, LZ4_COMPRESS_BOUND_SLOT_ADDRESS);
        assert_ne!(LZ4_FUNCTION_AT_PLUS_0X28, LZ4_PLUS_0X28_SLOT_ADDRESS);
        assert_ne!(LZ4_FUNCTION_AT_PLUS_0X30, LZ4_PLUS_0X30_SLOT_ADDRESS);
        assert_ne!(LZ4_FACTORY, LZ4_CONFIG_USE);
        assert_eq!(LZ4_COMPRESS_BOUND_WRAPPER, 0x146A_BEC70);
        assert_eq!(LZ4_FUNCTION_AT_PLUS_0X28, 0x146A_B3BD0);
        assert_eq!(LZ4_FUNCTION_AT_PLUS_0X30, 0x146A_B6540);
        assert_eq!(LZ4_FACTORY, 0x146A_B5400);
        assert_eq!(LZ4_CONFIG_USE, 0x1461_8048A);
    }
}
