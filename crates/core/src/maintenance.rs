//! Collection-wide, cross-process maintenance gate. Nested service calls reuse it.
use crate::{collections, Result};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};
thread_local! { static HELD: RefCell<BTreeMap<PathBuf, usize>> = const { RefCell::new(BTreeMap::new()) }; }
pub struct Guard {
    path: PathBuf,
    _file: Option<File>,
    _thread: PhantomData<Rc<()>>,
}
pub fn lock(root: &Path) -> Result<Guard> {
    lock_inner(root, None)
}
pub fn try_lock(root: &Path, timeout: std::time::Duration) -> Result<Guard> {
    lock_inner(root, Some(timeout))
}
fn lock_inner(root: &Path, timeout: Option<std::time::Duration>) -> Result<Guard> {
    let state = collections::state_dir(root)?;
    std::fs::create_dir_all(&state)?;
    let path = std::fs::canonicalize(state)?.join("maintenance.lock");
    let nested = HELD.with(|h| h.borrow().contains_key(&path));
    let file = if nested {
        None
    } else {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        if let Some(timeout) = timeout {
            let started = std::time::Instant::now();
            loop {
                match fs2::FileExt::try_lock_exclusive(&f) {
                    Ok(()) => break,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && started.elapsed() < timeout => std::thread::sleep(std::time::Duration::from_millis(10)),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Err(crate::Error::new("service_unavailable", "Collection is busy or under maintenance; retry the same request ID later")),
                    Err(e) => return Err(e.into()),
                }
            }
        } else {
            fs2::FileExt::lock_exclusive(&f)?;
        }
        crate::updates::recover(root)?;
        Some(f)
    };
    HELD.with(|h| *h.borrow_mut().entry(path.clone()).or_default() += 1);
    Ok(Guard {
        path,
        _file: file,
        _thread: PhantomData,
    })
}
impl Drop for Guard {
    fn drop(&mut self) {
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            let n = h.get_mut(&self.path).unwrap();
            *n -= 1;
            if *n == 0 {
                h.remove(&self.path);
            }
        });
    }
}
