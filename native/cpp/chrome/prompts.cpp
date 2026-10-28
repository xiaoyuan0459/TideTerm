// prompts 实现。

#include "cpp/chrome/prompts.h"
#include "cpp/i18n.h"

#include "cpp/bridge_shim.h"
#include "cpp/main_window.h"

#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QVBoxLayout>

namespace {

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

/// keyboard-interactive 多字段输入对话框。返回 false = 用户取消。
bool run_kb_dialog(QWidget *parent, const PromptInfo &p,
                   rust::Vec<rust::String> *out) {
    QDialog dlg(parent);
    dlg.setWindowTitle(i18n::tr("键盘交互验证"));
    auto *root = new QVBoxLayout(&dlg);
    if (!q(p.detail).isEmpty()) {
        auto *ins = new QLabel(q(p.detail), &dlg);
        ins->setWordWrap(true);
        root->addWidget(ins);
    }
    auto *form = new QFormLayout;
    std::vector<QLineEdit *> fields;
    for (size_t i = 0; i < p.fields.size(); ++i) {
        auto *e = new QLineEdit(&dlg);
        const bool echo = size_t(i) < p.echoes.size() ? bool(p.echoes[i]) : true;
        if (!echo) {
            e->setEchoMode(QLineEdit::Password);
        }
        form->addRow(
            q(p.fields[i]).isEmpty()
                ? i18n::tr("字段 %1").arg(i + 1)
                : q(p.fields[i]),
            e);
        fields.push_back(e);
    }
    root->addLayout(form);
    auto *buttons = new QDialogButtonBox(&dlg);
    buttons->addButton(QDialogButtonBox::Ok);
    buttons->addButton(QDialogButtonBox::Cancel);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dlg,
                     [&dlg] { dlg.accept(); });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dlg,
                     [&dlg] { dlg.reject(); });
    root->addWidget(buttons);
    if (dlg.exec() != QDialog::Accepted) {
        return false;
    }
    for (QLineEdit *e : fields) {
        out->push_back(rs(e->text()));
    }
    return true;
}

} // namespace

bool run_host_key_dialog(QWidget *parent, const PromptInfo &p) {
    const QString state_text =
        p.state == 2 ? i18n::tr("<b><font color='#d60'>! 该主机的密钥已改变——若你未预期"
                           "重装/换机，可能正在被中间人攻击，请谨慎核实！</font></b>")
                     : i18n::tr("首次连接该主机。请核对指纹后决定是否信任。");
    QMessageBox box(parent);
    box.setIcon(p.state == 2 ? QMessageBox::Warning : QMessageBox::Question);
    box.setWindowTitle(i18n::tr("确认主机密钥"));
    box.setTextFormat(Qt::RichText);
    box.setText(i18n::tr("<p><b>%1</b>（%2）</p>"
                               "<p>密钥类型：%3<br/>指纹 SHA-256：%4</p>"
                               "<p>%5</p>")
                    .arg(q(p.title), p.state == 2 ? i18n::tr("密钥变化")
                                                  : i18n::tr("未知主机"),
                         q(p.detail), q(p.fingerprint), state_text));
    box.addButton(i18n::tr("信任并连接"), QMessageBox::AcceptRole);
    box.addButton(i18n::tr("拒绝"), QMessageBox::RejectRole);
    box.exec();
    return box.clickedButton() != nullptr &&
           box.buttonRole(box.clickedButton()) == QMessageBox::AcceptRole;
}

void poll_prompts(MainWindow *win) {
    const PromptInfo p = sess_take_prompt();
    if (p.kind == 0) {
        return;
    }
    const QString uid = q(p.uid);
    if (p.kind == 1) {
        const bool accept = run_host_key_dialog(win, p);
        sess_answer_host_key(rust::String(uid.toUtf8().toStdString()), accept);
        win->note(accept ? i18n::tr("已信任主机 %1").arg(q(p.title))
                         : i18n::tr("已拒绝主机 %1").arg(q(p.title)));
        return;
    }
    // keyboard-interactive
    rust::Vec<rust::String> responses;
    if (run_kb_dialog(win, p, &responses)) {
        sess_answer_kb(rust::String(uid.toUtf8().toStdString()),
                       std::move(responses));
    } else {
        // 取消 = 全空应答（协议侧按失败处理，不断链）。
        rust::Vec<rust::String> blank;
        for (size_t i = 0; i < p.fields.size(); ++i) {
            blank.push_back(rs(QString()));
        }
        sess_answer_kb(rust::String(uid.toUtf8().toStdString()),
                       std::move(blank));
        win->note(i18n::tr("已取消验证"));
    }
}
