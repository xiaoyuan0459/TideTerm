//! sftp 域桥（ADR-014 按域拆桥第四域，Phase C ⑤）。
//!
//! 目录操作异步发起（结果经 `sftp_take_ops` 领取——UI 泵轮询）；传输队列按
//! 版本号拉取。`sftp_` 前缀跨桥全局唯一；只传 POD。

pub use crate::rust::sftp::{
    cancel_transfer, channel_ready, pause_transfer, resume_transfer, session_ready, sftp_close,
    sftp_download, sftp_list, sftp_mkdir, sftp_open, sftp_remove, sftp_rename, sftp_upload,
    take_ops, transfers_snapshot, transfers_version_value,
};

#[cxx::bridge]
pub mod sftp_ffi {
    /// 目录条目（list 结果行）。
    pub struct SftpEntry {
        pub name: String,
        pub path: String,
        pub is_dir: bool,
        pub size_bytes: u64,
        pub modified_unix: i64,
        pub permissions: u32,
        /// 数字 uid/gid（libssh2 stat 无用户名；空串 = 服务器未提供）。
        pub owner: String,
        pub group: String,
    }

    /// 一次异步操作的完成结果（kind：0=open 1=list 2=mkdir 3=remove 4=rename 5=transfer rejection）。
    pub struct SftpOpPod {
        pub kind: u8,
        pub uid: String,
        pub path: String,
        pub ok: bool,
        pub error: String,
        pub entries: Vec<SftpEntry>,
    }

    /// 传输队列行（进度/终态）。
    pub struct TransferPod {
        pub id: String,
        pub file_name: String,
        /// true = 上传（本地→远程）。
        pub up: bool,
        pub bytes_done: u64,
        pub bytes_total: u64,
        pub completed: bool,
        pub error: String,
    }

    extern "Rust" {
        /// 异步开 SFTP channel（仅 SSH 会话；完成静默，失败经 ops 队列回报）。
        fn sftp_open(uid: &str);
        fn sftp_close(uid: &str);
        /// channel 是否就绪（UI 状态显示）。
        fn sftp_channel_ready(uid: &str) -> bool;
        fn sftp_session_ready(uid: &str) -> bool;
        /// 异步列目录（结果经 take_ops）。
        fn sftp_list(uid: &str, path: &str);
        fn sftp_mkdir(uid: &str, path: &str);
        fn sftp_remove(uid: &str, path: &str);
        fn sftp_rename(uid: &str, from: &str, to: &str);
        /// 传输：上传本地文件 → 远程目录；下载远程文件 → 本地目录。
        fn sftp_upload(uid: &str, local_path: &str, remote_dir: &str);
        fn sftp_download(uid: &str, remote_path: &str, local_dir: &str, size_bytes: u64);
        /// 领取全部已完成操作结果（UI 泵轮询）。
        fn sftp_take_ops() -> Vec<SftpOpPod>;
        /// 传输队列快照 + 版本（版本不变可跳过拉取）。
        fn sftp_transfers() -> Vec<TransferPod>;
        fn sftp_transfers_version() -> u64;
        /// 取消传输（chunk 边界生效）。
        fn sftp_cancel_transfer(id: &str) -> bool;
        fn sftp_pause_transfer(id: &str) -> bool;
        fn sftp_resume_transfer(id: &str) -> bool;
    }
}

use crate::rust::sftp::{sftp_model, SftpOp};
use sftp_ffi::{SftpEntry, SftpOpPod, TransferPod};

pub(crate) fn sftp_channel_ready(uid: &str) -> bool {
    channel_ready(uid)
}

pub(crate) fn sftp_session_ready(uid: &str) -> bool {
    session_ready(uid)
}

pub(crate) fn sftp_take_ops() -> Vec<SftpOpPod> {
    take_ops()
        .into_iter()
        .map(|op: SftpOp| SftpOpPod {
            kind: op.kind as u8,
            uid: op.uid,
            path: op.path,
            ok: op.ok,
            error: op.error,
            entries: op
                .entries
                .into_iter()
                .map(|e| SftpEntry {
                    name: e.name,
                    path: e.path,
                    is_dir: e.is_dir,
                    size_bytes: e.size_bytes,
                    modified_unix: e.modified_unix,
                    permissions: e.permissions,
                    owner: e.owner.unwrap_or_default(),
                    group: e.group.unwrap_or_default(),
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn sftp_transfers() -> Vec<TransferPod> {
    transfers_snapshot()
        .into_iter()
        .map(|t| TransferPod {
            id: t.id,
            file_name: t.file_name,
            up: matches!(t.direction, sftp_model::TransferDirection::Upload),
            bytes_done: t.bytes_transferred,
            bytes_total: t.total_bytes,
            completed: t.completed,
            error: t.error.unwrap_or_default(),
        })
        .collect()
}

pub(crate) fn sftp_transfers_version() -> u64 {
    transfers_version_value()
}

pub(crate) fn sftp_cancel_transfer(id: &str) -> bool {
    cancel_transfer(id)
}

pub(crate) fn sftp_pause_transfer(id: &str) -> bool {
    pause_transfer(id)
}

pub(crate) fn sftp_resume_transfer(id: &str) -> bool {
    resume_transfer(id)
}
