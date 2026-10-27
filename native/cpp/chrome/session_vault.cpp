#include "cpp/chrome/session_vault.h"
#include "cpp/i18n.h"

#include <QInputDialog>
#include <QLineEdit>
#include <QMessageBox>

namespace {

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// 解锁循环：输错/节流提示后重试；取消返回 false。
bool prompt_unlock(QWidget *parent) {
    while (true) {
        bool ok = false;
        const QString mp = QInputDialog::getText(
            parent, i18n::tr("解锁凭据库"),
            i18n::tr("输入主密码以使用记住的密码："), QLineEdit::Password,
            QString(), &ok);
        if (!ok) {
            return false;
        }
        const rust::String err = vault_unlock(rs(mp));
        if (err.empty()) {
            return true;
        }
        QMessageBox::warning(parent, i18n::tr("解锁失败"), q(err));
    }
}

/// 设置向导：两次输入一致后创建 vault.json 并解锁。
bool prompt_setup(QWidget *parent) {
    while (true) {
        bool ok = false;
        const QString first = QInputDialog::getText(
            parent, i18n::tr("设置主密码"),
            i18n::tr("主密码（至少 8 位）用于加密记住的密码："),
            QLineEdit::Password, QString(), &ok);
        if (!ok) {
            return false;
        }
        if (first.size() < 8) {
            QMessageBox::warning(parent, i18n::tr("设置主密码"),
                                 i18n::tr("主密码至少需要 8 位。"));
            continue;
        }
        const QString again = QInputDialog::getText(
            parent, i18n::tr("确认主密码"), i18n::tr("再次输入主密码："),
            QLineEdit::Password, QString(), &ok);
        if (!ok) {
            return false;
        }
        if (again != first) {
            QMessageBox::warning(parent, i18n::tr("设置主密码"),
                                 i18n::tr("两次输入不一致，请重试。"));
            continue;
        }
        const rust::String err = vault_setup(rs(first));
        if (err.empty()) {
            return true;
        }
        QMessageBox::warning(parent, i18n::tr("设置失败"), q(err));
    }
}

} // namespace

namespace chrome {

bool ensure_vault_ready(QWidget *parent) {
    if (!vault_is_set()) {
        return prompt_setup(parent);
    }
    if (vault_is_unlocked()) {
        return true;
    }
    return prompt_unlock(parent);
}

void sync_saved_credentials(const SessCfg &cfg, const rust::String &id) {
    if (cfg.kind != 1) { // 仅 SSH
        return;
    }
    if (!cfg.ssh_remember) {
        vault_forget_session(id);
        return;
    }
    // 字段为空 = 表单未改动（表单永不回填密码），保留既有条目。
    if (!cfg.ssh_password.empty()) {
        const rust::String err = vault_store_password(id, cfg.ssh_password);
        if (!err.empty()) {
            qWarning("session vault store failed: %s", q(err).toUtf8().constData());
        }
    }
    if (!cfg.ssh_passphrase.empty()) {
        const rust::String err = vault_store_passphrase(id, cfg.ssh_passphrase);
        if (!err.empty()) {
            qWarning("session vault store failed: %s", q(err).toUtf8().constData());
        }
    }
}

uint32_t sess_open_with_vault(QWidget *parent, const rust::String &sess_id) {
    const uint32_t tab = sess_open(sess_id);
    if (tab != kVaultLocked) {
        return tab;
    }
    if (!prompt_unlock(parent)) {
        return kVaultLocked;
    }
    return sess_open(sess_id);
}

} // namespace chrome
