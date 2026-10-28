//! SFTP channel lifetime and initial remote path.
use super::*;

/// 异步开 channel：完成静默（后续操作可用）；失败以 list/操作错误形态回报。
pub fn sftp_open(uid: &str) {
    if !crate::rust::term::sftp_allowed(uid) {
        push_op(failed_op(OpKind::Open, uid, "", "该会话已关闭 SFTP"));
        return;
    }
    let uid = uid.to_string();
    let Some(protocol) = crate::rust::term::protocol_of_uid(&uid) else {
        push_op(failed_op(OpKind::Open, &uid, "", "会话不存在或尚未连接"));
        return;
    };
    if !protocol.is_alive() {
        push_op(failed_op(OpKind::Open, &uid, "", "SSH 会话已断开"));
        return;
    }
    if channel_ready(&uid) {
        push_op(SftpOp {
            kind: OpKind::Open,
            uid,
            path: String::new(),
            ok: true,
            error: String::new(),
            entries: Vec::new(),
        });
        return;
    }
    let Some(token) = lifecycle::begin_open(&uid) else {
        return; // 同一 uid 的开通道任务仍在进行
    };
    crate::rust::session::shared_runtime().spawn(async move {
        // Directory browsing also needs its own SSH connection. Readdir on a
        // terminal session competes with the PTY reader, especially for large
        // directories. Protocols without worker support retain the old path.
        let directory = match protocol.open_sftp_workers(1).await {
            Ok(mut workers) if !workers.is_empty() => Ok(workers.remove(0)),
            Ok(_) => protocol.open_sftp().await,
            Err(error) => {
                tracing::warn!("SFTP directory worker unavailable: {error}");
                protocol.open_sftp().await
            }
        };
        match directory {
            Ok(primary) => {
                if protocol.is_alive() && lifecycle::cache_channels(&uid, &token, vec![primary]) {
                    // UI 据此触发首个 list。
                    lifecycle::push_if_current(
                        &token,
                        SftpOp {
                            kind: OpKind::Open,
                            uid,
                            path: String::new(),
                            ok: true,
                            error: String::new(),
                            entries: Vec::new(),
                        },
                    );
                } else if protocol.is_alive() {
                    // 关闭后的迟到结果直接丢弃。
                } else {
                    lifecycle::open_failed(
                        &uid,
                        &token,
                        failed_op(OpKind::Open, &uid, "", "SSH 会话已断开"),
                    );
                }
            }
            Err(e) => lifecycle::open_failed(
                &uid,
                &token,
                failed_op(OpKind::Open, &uid, "", &e.to_string()),
            ),
        }
    });
}
