//! `ksee deploy`: build, then push the site (target/site, as served) to the
//! gh-pages branch of origin, for GitHub Pages. The branch is one commit,
//! replaced each time (the kernel is ~20 MB; no history of it to keep).
use std::process::Command;

use crate::build::{git, run};

pub fn deploy() {
    crate::build::build();
    let remote = git(&["remote", "get-url", "origin"]).unwrap_or_else(|| crate::fail("no origin remote to deploy to"));
    let head = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let dirty = if git(&["status", "--porcelain"]).unwrap_or_default().is_empty() { "" } else { " (with changes)" };

    let dir = std::env::temp_dir().join(format!("ksee-deploy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::or_fail(std::fs::create_dir_all(&dir), dir.display());
    eprintln!("  DEPLOY  {remote} gh-pages");
    run(Command::new("cp").arg("-r").arg(format!("{}/.", crate::build::SITE)).arg(&dir));
    // (No Jekyll: it would drop files that start with _.)
    crate::or_fail(std::fs::write(dir.join(".nojekyll"), ""), ".nojekyll");

    let d = dir.to_str().unwrap();
    let msg = format!("kSEE {head}{dirty}");
    for args in [&["init", "-q", "-b", "gh-pages"][..], &["add", "-A"], &["commit", "-q", "-m", &msg],
                 &["push", "-f", &remote, "gh-pages"]] {
        run(Command::new("git").arg("-C").arg(d).args(args));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
