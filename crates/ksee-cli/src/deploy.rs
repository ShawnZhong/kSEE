//! `ksee deploy`: build, then push the site (target/site, as served) to the
//! gh-pages branch of origin, for GitHub Pages. The branch is one commit,
//! replaced each time (the kernel is ~20 MB; no history of it to keep).
//! Pages can't send COOP/COEP; web/coi.js (a service worker) adds them.
use std::process::Command;

use crate::build::run;

fn git(args: &[&str]) -> String {
    let out = Command::new("git").args(args).output().unwrap_or_else(|e| crate::fail(&format!("git: {e}")));
    if !out.status.success() {
        crate::fail(&format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub fn deploy() {
    crate::build::build();
    let remote = git(&["remote", "get-url", "origin"]);
    let head = git(&["rev-parse", "--short", "HEAD"]);
    let dirty = if git(&["status", "--porcelain"]).is_empty() { "" } else { " (with changes)" };

    let dir = std::env::temp_dir().join(format!("ksee-deploy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", dir.display())));
    eprintln!("  DEPLOY  {remote} gh-pages");
    // -L: the files, not the links (to web/).
    run(Command::new("cp").arg("-rL").arg(format!("{}/.", crate::build::SITE)).arg(&dir));
    // (No Jekyll: it would drop files that start with _.)
    std::fs::write(dir.join(".nojekyll"), "").unwrap_or_else(|e| crate::fail(&format!(".nojekyll: {e}")));

    let d = dir.to_str().unwrap();
    let msg = format!("kSEE {head}{dirty}");
    for args in [&["init", "-q", "-b", "gh-pages"][..], &["add", "-A"], &["commit", "-q", "-m", &msg],
                 &["push", "-f", &remote, "gh-pages"]] {
        run(Command::new("git").arg("-C").arg(d).args(args));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
