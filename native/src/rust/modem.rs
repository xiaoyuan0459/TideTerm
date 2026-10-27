use crate::bridge::modem::modem_ffi::ModemProgress;
use tiderm_core::modem::{Kind, Request};
pub fn modem_start(tab: u32, kind: u8, receive: bool, paths: Vec<String>) -> String {
    super::term::try_with_tab(tab, |st| -> Result<(), String> {
        use super::store::SessionKind;
        if !matches!(
            st.config.kind,
            SessionKind::Ssh | SessionKind::Serial | SessionKind::Tcp
        ) {
            return Err("Modem requires SSH, Serial or TCP".into());
        }
        if st.config.kind == SessionKind::Tcp
            && st.config.tcp.as_ref().is_some_and(|tcp| tcp.data_mode != 1)
        {
            return Err("Modem requires TCP binary mode".into());
        }
        if st.config.kind == SessionKind::Serial
            && st
                .config
                .serial
                .as_ref()
                .is_some_and(|serial| serial.data_bits != 8 || serial.flow_control == 2)
        {
            return Err("Modem requires 8-bit Serial without XON/XOFF flow control".into());
        }
        let kind = match kind {
            0 => Kind::X,
            1 => Kind::Y,
            2 => Kind::Z,
            _ => return Err("unknown Modem protocol".into()),
        };
        if paths.is_empty()
            || paths.len() > 1024
            || paths.iter().any(|p| p.is_empty() || p.contains('\0'))
        {
            return Err("invalid transfer paths".into());
        }
        let request = if receive {
            if paths.len() != 1 {
                return Err("receive requires one destination".into());
            }
            Request::Receive {
                kind,
                path: paths[0].clone().into(),
            }
        } else {
            if matches!(kind, Kind::X) && paths.len() != 1 {
                return Err("XMODEM sends one file".into());
            }
            Request::Send {
                kind,
                paths: paths.into_iter().map(Into::into).collect(),
            }
        };
        st.host
            .start_modem(request, st.config.meta.options.modem_timeout_secs)
    })
    .unwrap_or_else(|| Err("session has closed".into()))
    .err()
    .unwrap_or_default()
}
pub fn modem_cancel(tab: u32) {
    super::term::try_with_tab(tab, |st| st.host.modem.cancel());
}
pub fn modem_progress(tab: u32) -> ModemProgress {
    super::term::try_with_tab(tab, |st| {
        let progress = st.host.modem.progress();
        ModemProgress {
            busy: st.host.modem.busy(),
            finished: progress.finished,
            bytes: progress.bytes,
            file: progress.file,
            error: progress.error,
        }
    })
    .unwrap_or_else(|| ModemProgress {
        busy: false,
        finished: true,
        bytes: 0,
        file: String::new(),
        error: "session has closed".into(),
    })
}
