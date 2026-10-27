//! Owned libssh2 channels for requests missing from ssh2's safe API.
//! Every C call holds ssh2's session mutex; clones share one channel owner.
use libssh2_sys as raw;
use std::ffi::{c_char, CString};
use std::io::{self, Read, Write};
use std::sync::Arc;

#[derive(Clone)]
pub struct Channel(Arc<Inner>);
struct Inner {
    ptr: *mut raw::LIBSSH2_CHANNEL,
    session: ssh2::Session,
    _forwarding: Option<super::forwarding::Forwarding>,
}
// The session owns the transport and its mutex serializes every pointer access.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

unsafe extern "C" {
    fn libssh2_channel_x11_req_ex(
        channel: *mut raw::LIBSSH2_CHANNEL,
        single_connection: i32,
        protocol: *const c_char,
        cookie: *const c_char,
        screen: i32,
    ) -> i32;
}

fn check(code: i32) -> Result<(), ssh2::Error> {
    if code < 0 {
        Err(ssh2::Error::from_errno(ssh2::ErrorCode::Session(code)))
    } else {
        Ok(())
    }
}
impl Channel {
    pub fn open(session: &ssh2::Session) -> Result<Self, ssh2::Error> {
        let mut guard = session.raw();
        let ptr = unsafe {
            raw::libssh2_channel_open_ex(
                &mut *guard,
                c"session".as_ptr(),
                7,
                2 << 20,
                32768,
                std::ptr::null(),
                0,
            )
        };
        if ptr.is_null() {
            let code = unsafe { raw::libssh2_session_last_errno(&mut *guard) };
            return Err(ssh2::Error::from_errno(ssh2::ErrorCode::Session(code)));
        }
        Ok(Self(Arc::new(Inner {
            ptr,
            session: session.clone(),
            _forwarding: None,
        })))
    }
    /// Callback transfers sole ownership of an accepted channel to this wrapper.
    pub(super) unsafe fn accepted(ptr: usize, session: &ssh2::Session) -> Self {
        Self(Arc::new(Inner {
            ptr: ptr as *mut _,
            session: session.clone(),
            _forwarding: None,
        }))
    }
    pub(super) fn retain_forwarding(&mut self, forwarding: super::forwarding::Forwarding) {
        // Called once before any channel clone is handed to a reader/writer.
        if let Some(inner) = Arc::get_mut(&mut self.0) {
            inner._forwarding = Some(forwarding);
        }
    }
    fn call(&self, f: impl FnOnce(*mut raw::LIBSSH2_CHANNEL) -> i32) -> Result<(), ssh2::Error> {
        let _guard = self.0.session.raw();
        check(f(self.0.ptr))
    }
    pub fn request_pty(&mut self, term: &str, cols: u16, rows: u16) -> Result<(), ssh2::Error> {
        self.call(|ptr| unsafe {
            raw::libssh2_channel_request_pty_ex(
                ptr,
                term.as_ptr().cast(),
                term.len() as u32,
                std::ptr::null(),
                0,
                cols.into(),
                rows.into(),
                0,
                0,
            )
        })
    }
    pub fn request_pty_size(
        &mut self,
        cols: u32,
        rows: u32,
        _: Option<u32>,
        _: Option<u32>,
    ) -> Result<(), ssh2::Error> {
        self.call(|ptr| unsafe {
            raw::libssh2_channel_request_pty_size_ex(ptr, cols as i32, rows as i32, 0, 0)
        })
    }
    pub fn request_agent(&self) -> Result<(), ssh2::Error> {
        self.call(|ptr| unsafe { raw::libssh2_channel_request_auth_agent(ptr) })
    }
    pub fn request_x11(&self, cookie: &str, screen: u32) -> Result<(), ssh2::Error> {
        let cookie = CString::new(cookie)?;
        self.call(|ptr| unsafe {
            libssh2_channel_x11_req_ex(
                ptr,
                0,
                c"MIT-MAGIC-COOKIE-1".as_ptr(),
                cookie.as_ptr(),
                screen as i32,
            )
        })
    }
    pub fn shell(&mut self) -> Result<(), ssh2::Error> {
        self.call(|ptr| unsafe {
            raw::libssh2_channel_process_startup(ptr, c"shell".as_ptr(), 5, std::ptr::null(), 0)
        })
    }
    pub fn close(&mut self) -> Result<(), ssh2::Error> {
        self.stop_forwarding();
        self.call(|ptr| unsafe { raw::libssh2_channel_close(ptr) })
    }
    pub fn stop_forwarding(&self) {
        if let Some(forwarding) = &self.0._forwarding {
            forwarding.stop();
        }
    }
    pub fn exit_status(&self) -> Result<i32, ssh2::Error> {
        let _guard = self.0.session.raw();
        Ok(unsafe { raw::libssh2_channel_get_exit_status(self.0.ptr) })
    }
}
fn io_result(code: isize) -> io::Result<usize> {
    if code < 0 {
        // Preserve ssh2's transient timeout/EAGAIN classification when SFTP
        // temporarily switches the shared session into blocking mode.
        return Err(ssh2::Error::from_errno(ssh2::ErrorCode::Session(code as i32)).into());
    }
    Ok(code as usize)
}
impl Read for Channel {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        let _guard = self.0.session.raw();
        io_result(unsafe {
            raw::libssh2_channel_read_ex(self.0.ptr, 0, data.as_mut_ptr().cast(), data.len())
        })
    }
}
impl Write for Channel {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let _guard = self.0.session.raw();
        io_result(unsafe {
            raw::libssh2_channel_write_ex(self.0.ptr, 0, data.as_ptr().cast(), data.len())
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self._forwarding.take();
        let _guard = self.session.raw();
        unsafe {
            raw::libssh2_channel_free(self.ptr);
        }
    }
}
