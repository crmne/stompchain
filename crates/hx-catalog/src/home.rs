//! Everything the program keeps on this machine sits in one directory named
//! after the program, so renaming the program moves all of it at once.
//!
//! Which would be nothing at all, except that the directory is where somebody's
//! library and setlists live, where every automatic backup of their pedal has
//! been written, and where the resources they went to some trouble to extract
//! sit. Changing the name without this does not fail: it starts up looking
//! somewhere new, finds nothing, and presents an empty library as though that
//! were the truth. Losing the lot quietly is worse than losing it loudly, and
//! neither is acceptable when the fix is one rename.

use std::path::{Path, PathBuf};

/// What the program used to be called, and what it is called now. Both appear
/// here and nowhere else; every other path in the program is built from the
/// current name.
const FORMER: &str = "stompchain";
const CURRENT: &str = "tonepush";

/// Bring what the old name owned across to the new one, answering with the
/// directories that moved.
///
/// Safe to run at every start, from either binary. It keys off the old
/// directory being there and the new one not, so a machine that has already
/// moved costs two directory checks, and one where somebody has started fresh
/// under the new name is left alone rather than merged into. Merging would have
/// to choose between two libraries that both claim the same slots, and there is
/// no answer to that which is right often enough to make silently.
pub fn adopt_former_name() -> Vec<PathBuf> {
    [data_home(), config_home()]
        .into_iter()
        .flatten()
        .filter_map(|base| adopt_in(&base))
        .collect()
}

/// The one move, given the directory both names live under.
///
/// Split out from the search for that directory so it can be tested against a
/// scratch directory. A test that reads `HOME` is a test that can reach the
/// real library, and this is the code that would move it.
fn adopt_in(base: &Path) -> Option<PathBuf> {
    let former = base.join(FORMER);
    let current = base.join(CURRENT);
    if !former.is_dir() || current.exists() {
        return None;
    }
    // Both sides share a parent, so this is a rename within one filesystem:
    // atomic, instant however large the library is, and either wholly done or
    // not begun. A copy would have to decide what to do when it ran out of room
    // half way through somebody's tones.
    std::fs::rename(&former, &current).ok()?;
    Some(current)
}

/// `~/.local/share`, by the same reckoning the library, the backups and the
/// extracted resources all use to find themselves.
fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))
}

/// `~/.config`, likewise for the config file.
fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory to play the two names out in. Nothing here reads
    /// `HOME`, so no test can reach the library on the machine running it.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tonepush-home-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn what_the_old_name_owned_comes_across() {
        let base = scratch("adopts");
        std::fs::create_dir_all(base.join(FORMER).join("library/objects")).unwrap();
        std::fs::write(base.join(FORMER).join("library/index.json"), b"{}").unwrap();

        assert_eq!(adopt_in(&base), Some(base.join(CURRENT)));
        assert!(base.join(CURRENT).join("library/objects").is_dir());
        assert_eq!(
            std::fs::read(base.join(CURRENT).join("library/index.json")).unwrap(),
            b"{}"
        );
        assert!(!base.join(FORMER).exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_library_already_under_the_new_name_is_not_merged_into() {
        let base = scratch("keeps");
        std::fs::create_dir_all(base.join(FORMER)).unwrap();
        std::fs::write(base.join(FORMER).join("index.json"), b"old").unwrap();
        std::fs::create_dir_all(base.join(CURRENT)).unwrap();
        std::fs::write(base.join(CURRENT).join("index.json"), b"new").unwrap();

        assert_eq!(adopt_in(&base), None);
        assert_eq!(
            std::fs::read(base.join(CURRENT).join("index.json")).unwrap(),
            b"new"
        );
        assert!(base.join(FORMER).exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_machine_that_never_knew_the_old_name_is_untouched() {
        let base = scratch("nothing");
        assert_eq!(adopt_in(&base), None);
        assert!(!base.join(CURRENT).exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// The second start, and every one after it.
    #[test]
    fn running_it_twice_moves_nothing_the_second_time() {
        let base = scratch("twice");
        std::fs::create_dir_all(base.join(FORMER).join("library")).unwrap();

        assert_eq!(adopt_in(&base), Some(base.join(CURRENT)));
        assert_eq!(adopt_in(&base), None);
        assert!(base.join(CURRENT).join("library").is_dir());

        let _ = std::fs::remove_dir_all(&base);
    }
}
