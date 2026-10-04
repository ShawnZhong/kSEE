//! `ksee deploy`: build, then push the site (target/site, as served) to the
//! gh-pages branch of origin, for GitHub Pages. The branch is one commit,
//! replaced each time (the kernel is ~20 MB; no history of it to keep).
use std::process::Command;

use crate::build::{run, stdout};

pub fn deploy() {
    crate::build::build();
    let git = |args: &[&str]| String::from_utf8_lossy(&stdout(Command::new("git").args(args))).trim().to_string();
    let remote = git(&["remote", "get-url", "origin"]);
    let head = git(&["rev-parse", "--short", "HEAD"]);
    let dirty = if git(&["status", "--porcelain"]).is_empty() { "" } else { " (with changes)" };

    let dir = std::env::temp_dir().join(format!("ksee-deploy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::or_fail(std::fs::create_dir_all(&dir), dir.display());
    eprintln!("  DEPLOY  {remote} gh-pages");
    // -L: the files, not the links (to web/).
    run(Command::new("cp").arg("-rL").arg(format!("{}/.", crate::build::SITE)).arg(&dir));
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
