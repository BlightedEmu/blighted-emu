// SPDX-License-Identifier: GPL-3.0-or-later
//! Puts the Blighted icon into the Windows program file. Other targets get nothing.

/// The resource script with the icon.
const RESOURCE_FILE: &str = "launcher.rc";
/// The icon file the resource script names.
const ICON_FILE: &str = "../../docs/images/favicon.ico";

fn main() {
    println!("cargo:rerun-if-changed={RESOURCE_FILE}");
    println!("cargo:rerun-if-changed={ICON_FILE}");
    if let Err(error) =
        embed_resource::compile(RESOURCE_FILE, embed_resource::NONE).manifest_required()
    {
        panic!("the launcher icon was not built into the program: {error}");
    }
}
