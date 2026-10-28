// QuickConnect 实现。

#include "cpp/chrome/quick_connect.h"

#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include "cpp/chrome/login_dialog.h"
#include "cpp/chrome/session_vault.h"

#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLineEdit>
#include <QMessageBox>
#include <QVBoxLayout>

uint32_t run_quick_connect(QWidget *parent, const QString &address) {
    const QuickTarget initial = context_parse_address(rust::String(address.toUtf8().toStdString()));
    if (!address.isEmpty() && !initial.ok) {
        QMessageBox::warning(parent, i18n::tr("快速连接"),
            i18n::tr(QString::fromUtf8(initial.error.data(), int(initial.error.size()))));
        return UINT32_MAX;
    }
    if (!address.isEmpty() && initial.kind == 0) return context_open_shell(initial.host);
    if (!address.isEmpty() && initial.kind != 1)
        return sess_quick_open(initial.kind, initial.host, initial.port, initial.user, rust::String());
    if (!address.isEmpty()) {
        const auto saved = sess_quick_saved(initial.host, initial.port, initial.user);
        if (!saved.empty()) {
            const uint32_t opened = chrome::sess_open_with_vault(parent, saved);
            if (opened != chrome::kVaultLocked && opened != UINT32_MAX) return opened;
        }
        auto cfg = sess_form_get(rust::Str(), 1);
        cfg.ssh_host = initial.host;
        cfg.ssh_port = initial.port ? initial.port : 22;
        cfg.ssh_user = initial.user;
        if (!run_login_dialog(parent, initial, &cfg)) return UINT32_MAX;
        return sess_open_draft(cfg);
    }
    QDialog dlg(parent);
    dlg.setWindowTitle(i18n::tr("快速连接"));
    auto *root = new QVBoxLayout(&dlg);
    auto *form = new QFormLayout;

    auto *proto = new QComboBox(&dlg);
    proto->addItems({QStringLiteral("SSH"), QStringLiteral("Telnet"),
                     QStringLiteral("TCP"), i18n::tr("串口")});
    form->addRow(i18n::tr("协议"), proto);
    auto *host = new QLineEdit(&dlg);
    host->setPlaceholderText(i18n::tr("host[:port] 或 端口路径"));
    form->addRow(i18n::tr("地址"), host);
    if (initial.ok) {
        static const int by_kind[] = {0, 0, 1, 3, 2};
        proto->setCurrentIndex(by_kind[initial.kind]);
        const QString h = QString::fromUtf8(initial.host.data(), int(initial.host.size()));
        host->setText((h.contains(':') ? QStringLiteral("[%1]").arg(h) : h) +
            (initial.port ? QStringLiteral(":%1").arg(initial.port) : QString()));
    }
    root->addLayout(form);

    auto *buttons = new QDialogButtonBox(&dlg);
    buttons->addButton(i18n::tr("连接"), QDialogButtonBox::AcceptRole);
    buttons->addButton(i18n::tr("取消"), QDialogButtonBox::RejectRole);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dlg, [&] {
        const QString scheme = QStringList{"ssh", "telnet", "tcp", "serial"}[proto->currentIndex()];
        const auto parsed = context_parse_address(rust::String((scheme + QStringLiteral("://") + host->text().trimmed()).toUtf8().toStdString()));
        if (!parsed.ok) QMessageBox::warning(&dlg, i18n::tr("快速连接"),
            i18n::tr(QString::fromUtf8(parsed.error.data(), int(parsed.error.size()))));
        else dlg.accept();
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dlg,
                     [&dlg] { dlg.reject(); });
    root->addWidget(buttons);

    if (dlg.exec() != QDialog::Accepted || host->text().trimmed().isEmpty()) {
        return UINT32_MAX;
    }

    const QString scheme = QStringList{"ssh", "telnet", "tcp", "serial"}[proto->currentIndex()];
    return run_quick_connect(parent, scheme + QStringLiteral("://") + host->text().trimmed());
}
