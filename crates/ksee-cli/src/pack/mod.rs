//! Pack the kernel for the page (called by `ksee build`, not a command of
//! its own): OUT/vmlinux, with the kernel's types and syscalls from DWARF
//! (see ksee_core::types) and without DWARF; and OUT/src, the sources its
//! probes refer to.

mod constants;
mod dwarf;
mod image;

use std::path::Path;

use ksee_core::{module, tables::Tables};

/// Write OUT/vmlinux (from KERNEL/vmlinux) and OUT/src (from KERNEL).
pub fn pack(kernel: &Path, out: &Path) {
    let image = crate::read(&kernel.join("vmlinux"));
    let mut types = dwarf::types(&image).unwrap_or_else(|e| crate::fail(&format!("DWARF: {e}")));
    let t = ksee_core::types::Types::parse(&types);
    for (name, value) in constants::constants(&kernel.join("usr/include")).into_iter().chain(constants::enumerators(&t)) {
        types += &format!("C\t{name}\t{value}\n");
    }
    let calls = image::syscalls(&image, &t);
    for (nr, name) in &calls {
        types += &format!("S\t{nr}\t{name}\n");
    }
    let packed = image::rewrite(&image, &types);
    std::fs::create_dir_all(out).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", out.display())));
    std::fs::write(out.join("vmlinux"), &packed).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", out.display())));

    // The sources, for the page's source view.
    let probes = String::from_utf8_lossy(&module::custom_section(&image, "ksee.probes")).into_owned();
    let tables = Tables::parse(&probes, &types);
    // (The functions the trace has whose parameters DWARF gave, by id: the
    // two agree on what a function is.)
    let known = tables.functions.keys().filter(|id| tables.types.params.contains_key(id)).count();
    let src = out.join("src");
    let _ = std::fs::remove_dir_all(&src);
    let (mut files, mut bytes) = (0, 0);
    for file in &tables.files {
        if file.starts_with('/') || file.contains("..") {
            continue; // outside the tree
        }
        if let Ok(data) = std::fs::read(kernel.join(file)) {
            let to = src.join(file);
            let _ = std::fs::create_dir_all(to.parent().unwrap());
            std::fs::write(&to, &data).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", to.display())));
            files += 1;
            bytes += data.len();
        }
    }
    eprintln!("{}: {} types, {} syscalls, parameters for {known} of {} functions, {} -> {} KiB; {files} sources, {} KiB",
              out.display(), types.lines().filter(|l| l.starts_with('T')).count(), calls.len(), tables.functions.len(),
              image.len() >> 10, packed.len() >> 10, bytes >> 10);
}
