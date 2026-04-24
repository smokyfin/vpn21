//! Shared tokio runtime for the FFI surface.

use once_cell::sync::OnceCell;
use tokio::runtime::Runtime;

static RT: OnceCell<Runtime> = OnceCell::new();

pub fn runtime() -> &'static Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("vpn21-ffi")
            .worker_threads(2)
            .build()
            .expect("build tokio runtime")
    })
}
